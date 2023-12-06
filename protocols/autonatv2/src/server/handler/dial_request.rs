use std::{
    convert::identity,
    io,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    channel::{mpsc, oneshot},
    AsyncRead, AsyncWrite, AsyncWriteExt, SinkExt, StreamExt,
};
use futures::future::FusedFuture;
use futures_bounded::FuturesSet;
use libp2p_core::{
    upgrade::{DeniedUpgrade, ReadyUpgrade},
    Multiaddr,
};
use libp2p_swarm::{
    handler::{ConnectionEvent, FullyNegotiatedInbound, ListenUpgradeError},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use rand_core::RngCore;

use crate::{
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    request_response::{
        DialDataRequest, DialDataResponse, DialRequest, DialResponse, Request, Response,
    },
    Nonce, REQUEST_UPGRADE,
};

#[derive(Debug, PartialEq)]
pub(crate) enum DialBack {
    Dial,
    DialBack,
    Ok,
}

#[derive(Debug)]
pub struct DialBackCommand {
    pub(crate) addr: Multiaddr,
    pub(crate) nonce: Nonce,
    pub(crate) back_channel: oneshot::Sender<DialBack>,
}

pub struct Handler<R> {
    observed_multiaddr: Multiaddr,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    dial_back_cmd_receiver: mpsc::Receiver<DialBackCommand>,
    inbound: FuturesSet<io::Result<()>>,
    rng: R,
}

impl<R> Handler<R>
where
    R: RngCore,
{
    pub(crate) fn new(observed_multiaddr: Multiaddr, rng: R) -> Self {
        let (dial_back_cmd_sender, dial_back_cmd_receiver) = mpsc::channel(10);
        Self {
            observed_multiaddr,
            dial_back_cmd_sender,
            dial_back_cmd_receiver,
            inbound: FuturesSet::new(Duration::from_secs(1000), 2),
            rng,
        }
    }
}

impl<R> ConnectionHandler for Handler<R>
where
    R: RngCore + Send + Clone + 'static,
{
    type FromBehaviour = ();

    type ToBehaviour = io::Result<DialBackCommand>;

    type InboundProtocol = ReadyUpgrade<StreamProtocol>;

    type OutboundProtocol = DeniedUpgrade;

    type InboundOpenInfo = ();

    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(REQUEST_UPGRADE, ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        match self.inbound.poll_unpin(cx) {
            Poll::Ready(Ok(Err(e))) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Err(e)));
            }
            Poll::Ready(Err(e)) => {
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Err(
                    io::Error::new(io::ErrorKind::TimedOut, e),
                )));
            }
            Poll::Ready(Ok(Ok(_))) => {}
            Poll::Pending => {}
        }
        if let Poll::Ready(Some(cmd)) = self.dial_back_cmd_receiver.poll_next_unpin(cx) {

            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Ok(cmd)));
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
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { error, .. }) => {
                tracing::debug!("inbound request failed: {:?}", error);
            }
            _ => {}
        }
    }

    fn connection_keep_alive(&self) -> bool {
        true
    }
}

enum HandleFail {
    InternalError(usize),
    RequestRejected,
    DialRefused,
    DialBack { idx: usize, err: DialBack },
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
                    DialBack::Dial => DialStatus::E_DIAL_ERROR,
                    DialBack::DialBack => DialStatus::E_DIAL_BACK_ERROR,
                    DialBack::Ok => DialStatus::OK,
                },
            },
        }
    }
}

async fn handle_request_internal(
    mut stream: impl AsyncRead + AsyncWrite + Unpin,
    observed_multiaddr: Multiaddr,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    mut rng: impl RngCore,
) -> Result<DialResponse, HandleFail> {
    let DialRequest { addrs, nonce } = match Request::read_from(&mut stream)
        .await
        .map_err(|_| HandleFail::InternalError(0))?
    {
        Request::Dial(dial_request) => dial_request,
        Request::Data(_) => {
            return Err(HandleFail::RequestRejected);
        }
    };
    for (idx, addr) in addrs.into_iter().enumerate() {
        if addr != observed_multiaddr {
            let dial_data_request = DialDataRequest::from_rng(idx, &mut rng);
            let mut rem_data = dial_data_request.num_bytes;
            Response::Data(dial_data_request)
                .write_into(&mut stream)
                .await
                .map_err(|_| HandleFail::InternalError(idx))?;
            while rem_data > 0 {
                let DialDataResponse { data_count } = match Request::read_from(&mut stream)
                    .await
                    .map_err(|_| HandleFail::InternalError(idx))?
                {
                    Request::Dial(_) => {
                        return Err(HandleFail::RequestRejected);
                    }
                    Request::Data(dial_data_response) => dial_data_response,
                };
                rem_data = rem_data.saturating_sub(data_count);
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
            .map_err(|_| HandleFail::InternalError(idx))?;
        if rx.is_terminated() {
            println!("is terminated");
        }
        let dial_back = rx.await.map_err(|e| {
            HandleFail::InternalError(idx)
        })?;
        if dial_back != DialBack::Ok {
            return Err(HandleFail::DialBack {
                idx,
                err: dial_back,
            });
        }
        return Ok(DialResponse {
            status: ResponseStatus::OK,
            addr_idx: idx,
            dial_status: DialStatus::OK,
        });
    }
    Err(HandleFail::DialRefused)
}

async fn handle_request(
    mut stream: impl AsyncRead + AsyncWrite + Unpin,
    observed_multiaddr: Multiaddr,
    dial_back_cmd_sender: mpsc::Sender<DialBackCommand>,
    rng: impl RngCore,
) -> io::Result<()> {
    let response =
        handle_request_internal(&mut stream, observed_multiaddr, dial_back_cmd_sender, rng)
            .await
            .unwrap_or_else(|e| e.into());
    Response::Dial(response).write_into(&mut stream).await?;
    stream.close().await?;
    Ok(())
}
