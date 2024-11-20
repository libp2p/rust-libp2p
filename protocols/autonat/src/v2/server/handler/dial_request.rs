use std::{
    convert::Infallible,
    io,
    task::{Context, Poll},
    time::Duration,
};

use either::Either;
use futures::{
    channel::{mpsc, oneshot},
    AsyncRead, AsyncWrite, SinkExt, StreamExt,
};
use futures_bounded::FuturesSet;
use libp2p_core::{
    upgrade::{DeniedUpgrade, ReadyUpgrade},
    Multiaddr,
};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    handler::{ConnectionEvent, FullyNegotiatedInbound, ListenUpgradeError},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use rand_core::RngCore;

use crate::v2::{
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    protocol::{Coder, DialDataRequest, DialRequest, DialResponse, Request, Response},
    server::behaviour::Event,
    Nonce, DIAL_REQUEST_PROTOCOL,
};

#[derive(Debug, PartialEq)]
pub(crate) enum DialBackStatus {
    /// Failure during dial
    DialErr,
    /// Failure during dial back
    DialBackErr,
}

#[derive(Debug)]
pub struct DialBackCommand {
    pub(crate) addr: Multiaddr,
    pub(crate) nonce: Nonce,
    pub(crate) back_channel: oneshot::Sender<Result<(), DialBackStatus>>,
}

pub struct Handler<R> {
    client_id: PeerId,
    observed_multiaddr: Multiaddr,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    dial_back_cmd_receiver: mpsc::Receiver<DialBackCommand>,
    inbound: FuturesSet<Event>,
    rng: R,
}

impl<R> Handler<R>
where
    R: RngCore,
{
    pub(crate) fn new(client_id: PeerId, observed_multiaddr: Multiaddr, rng: R) -> Self {
        let (dial_back_cmd_sender, dial_back_cmd_receiver) = mpsc::channel(10);
        Self {
            client_id,
            observed_multiaddr,
            dial_back_cmd_sender,
            dial_back_cmd_receiver,
            inbound: FuturesSet::new(Duration::from_secs(10), 10),
            rng,
        }
    }
}

impl<R> ConnectionHandler for Handler<R>
where
    R: RngCore + Send + Clone + 'static,
{
    type FromBehaviour = Infallible;
    type ToBehaviour = Either<DialBackCommand, Event>;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(DIAL_REQUEST_PROTOCOL), ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        loop {
            match self.inbound.poll_unpin(cx) {
                Poll::Ready(Ok(event)) => {
                    if let Err(e) = &event.result {
                        tracing::warn!("inbound request handle failed: {:?}", e);
                    }
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Right(
                        event,
                    )));
                }
                Poll::Ready(Err(e)) => {
                    tracing::warn!("inbound request handle timed out {e:?}");
                }
                Poll::Pending => break,
            }
        }
        if let Poll::Ready(Some(cmd)) = self.dial_back_cmd_receiver.poll_next_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Left(cmd)));
        }
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {}

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol, ..
            }) => {
                if self
                    .inbound
                    .try_push(handle_request(
                        protocol,
                        self.observed_multiaddr.clone(),
                        self.client_id,
                        self.dial_back_cmd_sender.clone(),
                        self.rng.clone(),
                    ))
                    .is_err()
                {
                    tracing::warn!(
                        "failed to push inbound request handler, too many requests in flight"
                    );
                }
            }
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { error, .. }) => {
                tracing::debug!("inbound request failed: {:?}", error);
            }
            _ => {}
        }
    }
}

enum HandleFail {
    InternalError(usize),
    RequestRejected,
    DialRefused,
    DialBack {
        idx: usize,
        result: Result<(), DialBackStatus>,
    },
}

impl From<HandleFail> for DialResponse {
    fn from(value: HandleFail) -> Self {
        match value {
            HandleFail::InternalError(addr_idx) => Self {
                status: ResponseStatus::E_INTERNAL_ERROR,
                addr_idx,
                dial_status: DialStatus::UNUSED,
            },
            HandleFail::RequestRejected => Self {
                status: ResponseStatus::E_REQUEST_REJECTED,
                addr_idx: 0,
                dial_status: DialStatus::UNUSED,
            },
            HandleFail::DialRefused => Self {
                status: ResponseStatus::E_DIAL_REFUSED,
                addr_idx: 0,
                dial_status: DialStatus::UNUSED,
            },
            HandleFail::DialBack { idx, result } => Self {
                status: ResponseStatus::OK,
                addr_idx: idx,
                dial_status: match result {
                    Err(DialBackStatus::DialErr) => DialStatus::E_DIAL_ERROR,
                    Err(DialBackStatus::DialBackErr) => DialStatus::E_DIAL_BACK_ERROR,
                    Ok(()) => DialStatus::OK,
                },
            },
        }
    }
}

async fn handle_request(
    stream: impl AsyncRead + AsyncWrite + Unpin,
    observed_multiaddr: Multiaddr,
    client: PeerId,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    rng: impl RngCore,
) -> Event {
    let mut coder = Coder::new(stream);
    let mut all_addrs = Vec::new();
    let mut tested_addr_opt = None;
    let mut data_amount = 0;
    let response = handle_request_internal(
        &mut coder,
        observed_multiaddr.clone(),
        dial_back_cmd_sender,
        rng,
        &mut all_addrs,
        &mut tested_addr_opt,
        &mut data_amount,
    )
    .await
    .unwrap_or_else(|e| e.into());
    let Some(tested_addr) = tested_addr_opt else {
        return Event {
            all_addrs,
            tested_addr: observed_multiaddr,
            client,
            data_amount,
            result: Err(io::Error::new(
                io::ErrorKind::Other,
                "client is not conformint to protocol. the tested address is not the observed address",
            )),
        };
    };
    if let Err(e) = coder.send(Response::Dial(response)).await {
        return Event {
            all_addrs,
            tested_addr,
            client,
            data_amount,
            result: Err(e),
        };
    }
    if let Err(e) = coder.close().await {
        return Event {
            all_addrs,
            tested_addr,
            client,
            data_amount,
            result: Err(e),
        };
    }
    Event {
        all_addrs,
        tested_addr,
        client,
        data_amount,
        result: Ok(()),
    }
}

async fn handle_request_internal<I>(
    coder: &mut Coder<I>,
    observed_multiaddr: Multiaddr,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    mut rng: impl RngCore,
    all_addrs: &mut Vec<Multiaddr>,
    tested_addrs: &mut Option<Multiaddr>,
    data_amount: &mut usize,
) -> Result<DialResponse, HandleFail>
where
    I: AsyncRead + AsyncWrite + Unpin,
{
    let DialRequest { mut addrs, nonce } = match coder
        .next()
        .await
        .map_err(|_| HandleFail::InternalError(0))?
    {
        Request::Dial(dial_request) => dial_request,
        Request::Data(_) => {
            return Err(HandleFail::RequestRejected);
        }
    };
    all_addrs.clone_from(&addrs);
    let idx = 0;
    let addr = addrs.pop().ok_or(HandleFail::DialRefused)?;
    *tested_addrs = Some(addr.clone());
    *data_amount = 0;
    if addr != observed_multiaddr {
        let dial_data_request = DialDataRequest::from_rng(idx, &mut rng);
        let mut rem_data = dial_data_request.num_bytes;
        coder
            .send(Response::Data(dial_data_request))
            .await
            .map_err(|_| HandleFail::InternalError(idx))?;
        while rem_data > 0 {
            let data_count = match coder
                .next()
                .await
                .map_err(|_e| HandleFail::InternalError(idx))?
            {
                Request::Dial(_) => {
                    return Err(HandleFail::RequestRejected);
                }
                Request::Data(dial_data_response) => dial_data_response.get_data_count(),
            };
            rem_data = rem_data.saturating_sub(data_count);
            *data_amount += data_count;
        }
    }
    let (back_channel, rx) = oneshot::channel();
    let dial_back_cmd = DialBackCommand {
        addr,
        nonce,
        back_channel,
    };
    dial_back_cmd_sender
        .clone()
        .send(dial_back_cmd)
        .await
        .map_err(|_| HandleFail::DialBack {
            idx,
            result: Err(DialBackStatus::DialErr),
        })?;

    let dial_back = rx.await.map_err(|_e| HandleFail::InternalError(idx))?;
    if let Err(err) = dial_back {
        return Err(HandleFail::DialBack {
            idx,
            result: Err(err),
        });
    }
    Ok(DialResponse {
        status: ResponseStatus::OK,
        addr_idx: idx,
        dial_status: DialStatus::OK,
    })
}
