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
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedOutbound, OutboundUpgradeSend,
        ProtocolsChange,
    },
    ConnectionHandler, ConnectionHandlerEvent, Stream, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol,
};
use std::{
    collections::VecDeque,
    convert::identity,
    io,
    iter::{once, repeat},
    sync::Arc,
    task::{Context, Poll},
};

use crate::client::behaviour::Error;
use crate::{
    client::behaviour::Event,
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    request_response::{
        Coder, DialDataRequest, DialDataResponse, DialRequest, DialResponse, Request, Response,
        DATA_FIELD_LEN_UPPER_BOUND, DATA_LEN_LOWER_BOUND, DATA_LEN_UPPER_BOUND,
    },
    REQUEST_PROTOCOL_NAME,
};

use super::{DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS};

#[derive(Debug, thiserror::Error)]
pub(crate) enum InternalError {
    #[error("io error")]
    Io(#[from] io::Error),
    #[error("invalid referenced address index: {index} (max number of addr: {max})")]
    InvalidReferencedAddress { index: usize, max: usize },
    #[error("data request too large: {len} (max: {max})")]
    DataRequestTooLarge { len: usize, max: usize },
    #[error("data request too small: {len} (min: {min})")]
    DataRequestTooSmall { len: usize, min: usize },
    #[error("timeout")]
    Timeout(#[from] futures_bounded::Timeout),
    #[error("server rejected dial request")]
    ServerRejectedDialRequest,
    #[error("server chose not to dial any provided address")]
    ServerChoseNotToDialAnyAddress,
    #[error("server ran into an internal error")]
    InternalServer,
    #[error("server did not respond correctly to dial request")]
    InvalidResponse,
    #[error("server was unable to connect to address: {addr:?}")]
    UnableToConnectOnSelectedAddress { addr: Option<Multiaddr> },
    #[error("server experienced failure during dial back on address: {addr:?}")]
    FailureDuringDialBack { addr: Option<Multiaddr> },
    #[error("error during substream upgrad")]
    Substream(
        #[from] StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
    ),
}

#[derive(Debug)]
pub struct TestEnd {
    pub(crate) dial_request: DialRequest,
    pub(crate) reachable_addr: Multiaddr,
}

#[derive(Debug)]
pub enum ToBehaviour {
    TestCompleted(Result<TestEnd, Error>),
    StatusUpdate(Event),
    PeerHasServerSupport,
}

pub struct Handler {
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,
    outbound: futures_bounded::FuturesSet<Result<TestEnd, crate::client::behaviour::Error>>,
    queued_streams: VecDeque<
        oneshot::Sender<
            Result<
                Stream,
                StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
            >,
        >,
    >,
    status_update_rx: mpsc::Receiver<Event>,
    status_update_tx: mpsc::Sender<Event>,
    server: PeerId,
}

impl Handler {
    pub(crate) fn new(server: PeerId) -> Self {
        let (status_update_tx, status_update_rx) = mpsc::channel(10);
        Self {
            queued_events: VecDeque::new(),
            outbound: FuturesSet::new(DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS),
            queued_streams: VecDeque::default(),
            status_update_tx,
            status_update_rx,
            server,
        }
    }

    fn perform_request(&mut self, req: DialRequest) {
        let (tx, rx) = oneshot::channel();
        self.queued_streams.push_back(tx);
        self.queued_events
            .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(REQUEST_PROTOCOL_NAME), ()),
            });
        if self
            .outbound
            .try_push(start_substream_handle(
                self.server,
                req,
                rx,
                self.status_update_tx.clone(),
            ))
            .is_err()
        {
            tracing::debug!("Dial request dropped, too many requests in flight");
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = DialRequest;
    type ToBehaviour = ToBehaviour;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }
        if let Poll::Ready(m) = self.outbound.poll_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                ToBehaviour::TestCompleted(
                    m.map_err(InternalError::Timeout)
                        .map_err(Into::into)
                        .and_then(identity)
                        .map_err(Into::into),
                ),
            ));
        }
        if let Poll::Ready(Some(status_update)) = self.status_update_rx.poll_next_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                ToBehaviour::StatusUpdate(status_update),
            ));
        }
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        self.perform_request(event);
    }

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
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, .. }) => {
                tracing::debug!("Dial request failed: {}", error);
                match self.queued_streams.pop_front() {
                    Some(stream_tx) => {
                        if stream_tx.send(Err(error)).is_err() {
                            tracing::warn!("Failed to send stream to dead handler");
                        }
                    }
                    None => {
                        tracing::warn!(
                            "Opened unexpected substream without a pending dial request"
                        );
                    }
                }
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => match self.queued_streams.pop_front() {
                Some(stream_tx) => {
                    if stream_tx.send(Ok(protocol)).is_err() {
                        tracing::warn!("Failed to send stream to dead handler");
                    }
                }
                None => {
                    tracing::warn!("Opened unexpected substream without a pending dial request");
                }
            },
            ConnectionEvent::RemoteProtocolsChange(ProtocolsChange::Added(mut added)) => {
                if added.any(|p| p.as_ref() == REQUEST_PROTOCOL_NAME) {
                    self.queued_events
                        .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                            ToBehaviour::PeerHasServerSupport,
                        ));
                }
            }
            _ => {}
        }
    }
}

async fn start_substream_handle(
    server: PeerId,
    dial_request: DialRequest,
    substream_recv: oneshot::Receiver<
        Result<
            Stream,
            StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
        >,
    >,
    mut status_update_tx: mpsc::Sender<Event>,
) -> Result<TestEnd, crate::client::behaviour::Error> {
    let substream = match substream_recv.await {
        Ok(Ok(substream)) => substream,
        Ok(Err(err)) => return Err(InternalError::from(err).into()),
        Err(_) => return Err(InternalError::InternalServer.into()),
    };
    let mut data_amount = 0;
    let mut checked_addr_idx = None;
    let addrs = dial_request.addrs.clone();
    assert_ne!(addrs, vec![]);
    let res = handle_substream(
        dial_request,
        substream,
        &mut data_amount,
        &mut checked_addr_idx,
    )
    .await
    .map_err(Arc::new)
    .map_err(crate::client::behaviour::Error::from);
    let status_update = Event {
        tested_addr: checked_addr_idx.and_then(|idx| addrs.get(idx).cloned()),
        data_amount,
        server,
        result: res.as_ref().map(|_| ()).map_err(|e| e.duplicate()),
    };
    let _ = status_update_tx.send(status_update).await;
    res
}

async fn handle_substream(
    dial_request: DialRequest,
    substream: impl AsyncRead + AsyncWrite + Unpin,
    data_amount: &mut usize,
    checked_addr_idx: &mut Option<usize>,
) -> Result<TestEnd, InternalError> {
    let mut coder = Coder::new(substream);
    coder.send(Request::Dial(dial_request.clone())).await?;
    match coder.next().await? {
        Response::Data(DialDataRequest {
            addr_idx,
            num_bytes,
        }) => {
            if addr_idx >= dial_request.addrs.len() {
                return Err(InternalError::InvalidReferencedAddress {
                    index: addr_idx,
                    max: dial_request.addrs.len(),
                });
            }
            if num_bytes > DATA_LEN_UPPER_BOUND {
                return Err(InternalError::DataRequestTooLarge {
                    len: num_bytes,
                    max: DATA_LEN_UPPER_BOUND,
                });
            }
            if num_bytes < DATA_LEN_LOWER_BOUND {
                return Err(InternalError::DataRequestTooSmall {
                    len: num_bytes,
                    min: DATA_LEN_LOWER_BOUND,
                });
            }
            *checked_addr_idx = Some(addr_idx);
            send_aap_data(&mut coder, num_bytes, data_amount).await?;
            if let Response::Dial(dial_response) = coder.next().await? {
                *checked_addr_idx = Some(dial_response.addr_idx);
                coder.close().await?;
                test_end_from_dial_response(dial_request, dial_response)
            } else {
                Err(InternalError::InternalServer)
            }
        }
        Response::Dial(dial_response) => {
            *checked_addr_idx = Some(dial_response.addr_idx);
            coder.close().await?;
            test_end_from_dial_response(dial_request, dial_response)
        }
    }
}

fn test_end_from_dial_response(
    req: DialRequest,
    resp: DialResponse,
) -> Result<TestEnd, InternalError> {
    if resp.addr_idx >= req.addrs.len() {
        return Err(InternalError::InvalidReferencedAddress {
            index: resp.addr_idx,
            max: req.addrs.len(),
        });
    }
    match (resp.status, resp.dial_status) {
        (ResponseStatus::E_REQUEST_REJECTED, _) => Err(InternalError::ServerRejectedDialRequest),
        (ResponseStatus::E_DIAL_REFUSED, _) => Err(InternalError::ServerChoseNotToDialAnyAddress),
        (ResponseStatus::E_INTERNAL_ERROR, _) => Err(InternalError::InternalServer),
        (ResponseStatus::OK, DialStatus::UNUSED) => Err(InternalError::InvalidResponse),
        (ResponseStatus::OK, DialStatus::E_DIAL_ERROR) => {
            Err(InternalError::UnableToConnectOnSelectedAddress {
                addr: req.addrs.get(resp.addr_idx).cloned(),
            })
        }
        (ResponseStatus::OK, DialStatus::E_DIAL_BACK_ERROR) => {
            Err(InternalError::FailureDuringDialBack {
                addr: req.addrs.get(resp.addr_idx).cloned(),
            })
        }
        (ResponseStatus::OK, DialStatus::OK) => req
            .addrs
            .get(resp.addr_idx)
            .ok_or(InternalError::InvalidReferencedAddress {
                index: resp.addr_idx,
                max: req.addrs.len(),
            })
            .cloned()
            .map(|reachable_addr| TestEnd {
                dial_request: req,
                reachable_addr,
            }),
    }
}

async fn send_aap_data<I>(
    substream: &mut Coder<I>,
    num_bytes: usize,
    data_amount: &mut usize,
) -> io::Result<()>
where
    I: AsyncWrite + Unpin,
{
    let count_full = num_bytes / DATA_FIELD_LEN_UPPER_BOUND;
    let partial_len = num_bytes % DATA_FIELD_LEN_UPPER_BOUND;
    for (data_count, req) in repeat(DATA_FIELD_LEN_UPPER_BOUND)
        .take(count_full)
        .chain(once(partial_len))
        .filter(|e| *e > 0)
        .map(|data_count| {
            (
                data_count,
                Request::Data(
                    DialDataResponse::new(data_count).expect("data count is unexpectedly too big"),
                ),
            )
        })
    {
        *data_amount += data_count;
        substream.send(req).await?;
    }
    Ok(())
}
