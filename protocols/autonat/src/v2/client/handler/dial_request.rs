use futures::{channel::oneshot, AsyncRead, AsyncWrite};
use futures_bounded::FuturesSet;
use libp2p_core::{
    upgrade::{DeniedUpgrade, ReadyUpgrade},
    Multiaddr,
};

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
    io,
    iter::{once, repeat},
    task::{Context, Poll},
    time::Duration,
};

use crate::v2::{
    client::behaviour::Error,
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    protocol::{
        Coder, DialDataRequest, DialDataResponse, DialRequest, DialResponse, Request, Response,
        DATA_FIELD_LEN_UPPER_BOUND, DATA_LEN_LOWER_BOUND, DATA_LEN_UPPER_BOUND,
    },
    DIAL_REQUEST_PROTOCOL,
};

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
}

#[derive(Debug)]
pub struct InternalStatusUpdate {
    pub(crate) tested_addr: Option<Multiaddr>,
    pub(crate) bytes_sent: usize,
    pub result: Result<TestEnd, Error>,
    pub(crate) server_no_support: bool,
}

#[derive(Debug)]
pub struct TestEnd {
    pub(crate) dial_request: DialRequest,
    pub(crate) reachable_addr: Multiaddr,
}

#[derive(Debug)]
pub enum ToBehaviour {
    TestCompleted(InternalStatusUpdate),
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
    outbound: futures_bounded::FuturesSet<InternalStatusUpdate>,
    queued_streams: VecDeque<
        oneshot::Sender<
            Result<
                Stream,
                StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
            >,
        >,
    >,
}

impl Handler {
    pub(crate) fn new() -> Self {
        Self {
            queued_events: VecDeque::new(),
            outbound: FuturesSet::new(Duration::from_secs(10), 10),
            queued_streams: VecDeque::default(),
        }
    }

    fn perform_request(&mut self, req: DialRequest) {
        let (tx, rx) = oneshot::channel();
        self.queued_streams.push_back(tx);
        self.queued_events
            .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(DIAL_REQUEST_PROTOCOL), ()),
            });
        if self
            .outbound
            .try_push(start_stream_handle(req, rx))
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
            let status_update = match m {
                Ok(ok) => ok,
                Err(_) => InternalStatusUpdate {
                    tested_addr: None,
                    bytes_sent: 0,
                    result: Err(Error {
                        internal: InternalError::Io(io::Error::from(io::ErrorKind::TimedOut)),
                    }),
                    server_no_support: false,
                },
            };
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                ToBehaviour::TestCompleted(status_update),
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
                        let _ = stream_tx.send(Err(error));
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
                if added.any(|p| p.as_ref() == DIAL_REQUEST_PROTOCOL) {
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

async fn start_stream_handle(
    dial_request: DialRequest,
    stream_recv: oneshot::Receiver<
        Result<
            Stream,
            StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
        >,
    >,
) -> InternalStatusUpdate {
    let mut server_no_support = false;
    let substream_result = match stream_recv.await {
        Ok(Ok(substream)) => Ok(substream),
        Ok(Err(StreamUpgradeError::Io(io))) => Err(InternalError::from(io).into()),
        Ok(Err(StreamUpgradeError::Timeout)) => {
            Err(InternalError::Io(io::Error::from(io::ErrorKind::TimedOut)).into())
        }
        Ok(Err(StreamUpgradeError::Apply(upgrade_error))) => void::unreachable(upgrade_error),
        Ok(Err(StreamUpgradeError::NegotiationFailed)) => {
            server_no_support = true;
            Err(
                InternalError::Io(io::Error::new(io::ErrorKind::Other, "negotiation failed"))
                    .into(),
            )
        }
        Err(_) => Err(InternalError::InternalServer.into()),
    };
    let substream = match substream_result {
        Ok(substream) => substream,
        Err(err) => {
            let status_update = InternalStatusUpdate {
                tested_addr: None,
                bytes_sent: 0,
                result: Err(err),
                server_no_support,
            };
            return status_update;
        }
    };
    let mut data_amount = 0;
    let mut checked_addr_idx = None;
    let addrs = dial_request.addrs.clone();
    assert_ne!(addrs, vec![]);
    let result = handle_stream(
        dial_request,
        substream,
        &mut data_amount,
        &mut checked_addr_idx,
    )
    .await
    .map_err(crate::v2::client::behaviour::Error::from);
    InternalStatusUpdate {
        tested_addr: checked_addr_idx.and_then(|idx| addrs.get(idx).cloned()),
        bytes_sent: data_amount,
        result,
        server_no_support,
    }
}

async fn handle_stream(
    dial_request: DialRequest,
    stream: impl AsyncRead + AsyncWrite + Unpin,
    data_amount: &mut usize,
    checked_addr_idx: &mut Option<usize>,
) -> Result<TestEnd, InternalError> {
    let mut coder = Coder::new(stream);
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
    stream: &mut Coder<I>,
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
        stream.send(req).await?;
    }
    Ok(())
}
