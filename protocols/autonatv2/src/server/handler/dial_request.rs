use std::{
    io,
    sync::Arc,
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

use crate::{
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    request_response::{DialDataRequest, DialRequest, DialResponse, Request, Response},
    Nonce,
};
use crate::{request_response::Coder, REQUEST_PROTOCOL_NAME};

#[derive(Clone, Debug)]
pub struct StatusUpdate {
    pub all_addrs: Vec<Multiaddr>,
    pub tested_addr: Option<Multiaddr>,
    pub client: PeerId,
    pub data_amount: usize,
    pub result: Result<(), Arc<io::Error>>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum DialBackStatus {
    /// Failure during dial
    DialErr,
    /// Failure during dial back
    DialBackErr,
    Ok,
}

#[derive(Debug)]
pub struct DialBackCommand {
    pub(crate) addr: Multiaddr,
    pub(crate) nonce: Nonce,
    pub(crate) back_channel: oneshot::Sender<DialBackStatus>,
}

pub struct Handler<R> {
    client_id: PeerId,
    observed_multiaddr: Multiaddr,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    dial_back_cmd_receiver: mpsc::Receiver<DialBackCommand>,
    status_update_sender: mpsc::Sender<StatusUpdate>,
    status_update_receiver: mpsc::Receiver<StatusUpdate>,
    inbound: FuturesSet<Result<(), Arc<io::Error>>>,
    rng: R,
}

impl<R> Handler<R>
where
    R: RngCore,
{
    pub(crate) fn new(client_id: PeerId, observed_multiaddr: Multiaddr, rng: R) -> Self {
        let (dial_back_cmd_sender, dial_back_cmd_receiver) = mpsc::channel(10);
        let (status_update_sender, status_update_receiver) = mpsc::channel(10);
        Self {
            client_id,
            observed_multiaddr,
            dial_back_cmd_sender,
            dial_back_cmd_receiver,
            status_update_sender,
            status_update_receiver,
            inbound: FuturesSet::new(Duration::from_secs(10), 10),
            rng,
        }
    }
}

impl<R> ConnectionHandler for Handler<R>
where
    R: RngCore + Send + Clone + 'static,
{
    type FromBehaviour = ();

    type ToBehaviour = Either<Result<DialBackCommand, Arc<io::Error>>, StatusUpdate>;

    type InboundProtocol = ReadyUpgrade<StreamProtocol>;

    type OutboundProtocol = DeniedUpgrade;

    type InboundOpenInfo = ();

    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(REQUEST_PROTOCOL_NAME), ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        match self.inbound.poll_unpin(cx) {
            Poll::Ready(Ok(Err(e))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Left(Err(
                    e,
                ))));
            }
            Poll::Ready(Err(e)) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Left(Err(
                    Arc::new(io::Error::new(io::ErrorKind::TimedOut, e)),
                ))));
            }
            Poll::Ready(Ok(Ok(_))) => {}
            Poll::Pending => {}
        }
        if let Poll::Ready(Some(cmd)) = self.dial_back_cmd_receiver.poll_next_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Left(Ok(
                cmd,
            ))));
        }
        if let Poll::Ready(Some(status_update)) = self.status_update_receiver.poll_next_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Either::Right(
                status_update,
            )));
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
                    .try_push(start_handle_request(
                        protocol,
                        self.observed_multiaddr.clone(),
                        self.client_id,
                        self.dial_back_cmd_sender.clone(),
                        self.status_update_sender.clone(),
                        self.rng.clone(),
                    ))
                    .is_err()
                {
                    tracing::warn!(
                        "failed to push inbound request handler, too many requests in flight"
                    );
                }
            }
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
    DialBack { idx: usize, err: DialBackStatus },
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
            HandleFail::DialBack { idx, err } => Self {
                status: ResponseStatus::OK,
                addr_idx: idx,
                dial_status: match err {
                    DialBackStatus::DialErr => DialStatus::E_DIAL_ERROR,
                    DialBackStatus::DialBackErr => DialStatus::E_DIAL_BACK_ERROR,
                    DialBackStatus::Ok => DialStatus::OK,
                },
            },
        }
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
    *all_addrs = addrs.clone();
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
            err: DialBackStatus::DialErr,
        })?;

    // TODO: add timeout
    let dial_back = rx.await.map_err(|_e| HandleFail::InternalError(idx))?;
    if dial_back != DialBackStatus::Ok {
        return Err(HandleFail::DialBack {
            idx,
            err: dial_back,
        });
    }
    Ok(DialResponse {
        status: ResponseStatus::OK,
        addr_idx: idx,
        dial_status: DialStatus::OK,
    })
}

async fn handle_request(
    stream: impl AsyncRead + AsyncWrite + Unpin,
    observed_multiaddr: Multiaddr,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    rng: impl RngCore,
    all_addrs: &mut Vec<Multiaddr>,
    tested_addrs: &mut Option<Multiaddr>,
    data_amount: &mut usize,
) -> io::Result<()> {
    let mut coder = Coder::new(stream);
    let response = handle_request_internal(
        &mut coder,
        observed_multiaddr,
        dial_back_cmd_sender,
        rng,
        all_addrs,
        tested_addrs,
        data_amount,
    )
    .await
    .unwrap_or_else(|e| e.into());
    coder.send(Response::Dial(response)).await?;
    coder.close().await?;
    Ok(())
}

async fn start_handle_request(
    stream: impl AsyncRead + AsyncWrite + Unpin,
    observed_multiaddr: Multiaddr,
    client: PeerId,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    mut status_update_sender: mpsc::Sender<StatusUpdate>,
    rng: impl RngCore,
) -> Result<(), Arc<io::Error>> {
    let mut all_addrs = Vec::new();
    let mut tested_addrs = None;
    let mut data_amount = 0;
    let res = handle_request(
        stream,
        observed_multiaddr,
        dial_back_cmd_sender,
        rng,
        &mut all_addrs,
        &mut tested_addrs,
        &mut data_amount,
    )
    .await
    .map_err(Arc::new);
    let status_update = StatusUpdate {
        all_addrs,
        tested_addr: tested_addrs,
        client,
        data_amount,
        result: res.as_ref().map_err(Arc::clone).map(|_| ()),
    };
    let _ = status_update_sender.send(status_update).await;
    res
}
