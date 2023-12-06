use futures::{channel::oneshot, AsyncRead, AsyncWrite, AsyncWriteExt};
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
    convert::identity,
    io,
    iter::{once, repeat},
    task::{Context, Poll},
};

use crate::request_response::Coder;
use crate::{
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    request_response::{
        DialDataRequest, DialDataResponse, DialRequest, DialResponse, Request, Response,
        DATA_FIELD_LEN_UPPER_BOUND, DATA_LEN_LOWER_BOUND, DATA_LEN_UPPER_BOUND,
    },
    REQUEST_PROTOCOL_NAME, REQUEST_UPGRADE,
};

use super::{DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS};

#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    SubstreamError(
        #[from] StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
    ),
}

#[derive(Debug)]
pub struct TestEnd {
    pub(crate) dial_request: DialRequest,
    pub(crate) suspicious_addr: Vec<Multiaddr>,
    pub(crate) reachable_addr: Multiaddr,
}

#[derive(Debug)]
pub enum ToBehaviour {
    TestCompleted(Result<TestEnd, Error>),
    PeerHasServerSupport,
}

#[derive(Debug)]
pub enum FromBehaviour {
    PerformRequest(DialRequest),
}

pub struct Handler {
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,
    outbound: futures_bounded::FuturesSet<Result<TestEnd, Error>>,
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
            outbound: FuturesSet::new(DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS),
            queued_streams: VecDeque::default(),
        }
    }

    fn perform_request(&mut self, req: DialRequest) {
        println!("{req:?}");
        let (tx, rx) = oneshot::channel();
        self.queued_streams.push_back(tx);
        self.queued_events
            .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(REQUEST_UPGRADE, ()),
            });
        if self
            .outbound
            .try_push(start_substream_handle(req, rx))
            .is_err()
        {
            tracing::debug!("Dial request dropped, too many requests in flight");
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = FromBehaviour;

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
                ToBehaviour::TestCompleted(m.map_err(Error::Timeout).and_then(identity)),
            ));
        }
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            FromBehaviour::PerformRequest(req) => {
                self.perform_request(req);
            }
        }
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
                        if let Err(_) = stream_tx.send(Err(error)) {
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
                    if let Err(_) = stream_tx.send(Ok(protocol)) {
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
    dial_request: DialRequest,
    substream_recv: oneshot::Receiver<
        Result<
            Stream,
            StreamUpgradeError<<ReadyUpgrade<StreamProtocol> as OutboundUpgradeSend>::Error>,
        >,
    >,
) -> Result<TestEnd, Error> {
    match substream_recv.await {
        Ok(Ok(substream)) => handle_substream(dial_request, substream).await,
        Ok(Err(err)) => Err(Error::from(err)),
        Err(_) => Err(Error::InternalServer),
    }
}

async fn handle_substream(
    dial_request: DialRequest,
    substream: impl AsyncRead + AsyncWrite + Unpin,
) -> Result<TestEnd, Error> {
    let mut coder = Coder::new(substream);
    coder
        .send_request(Request::Dial(dial_request.clone()))
        .await?;
    let mut suspicious_addr = Vec::new();
    loop {
        match coder.next_response().await? {
            Response::Data(DialDataRequest {
                addr_idx,
                num_bytes,
            }) => {
                if addr_idx >= dial_request.addrs.len() {
                    return Err(Error::InvalidReferencedAddress {
                        index: addr_idx,
                        max: dial_request.addrs.len(),
                    });
                }
                if num_bytes > DATA_LEN_UPPER_BOUND {
                    return Err(Error::DataRequestTooLarge {
                        len: num_bytes,
                        max: DATA_LEN_UPPER_BOUND,
                    });
                }
                if num_bytes < DATA_LEN_LOWER_BOUND {
                    return Err(Error::DataRequestTooSmall {
                        len: num_bytes,
                        min: DATA_LEN_LOWER_BOUND,
                    });
                }
                match dial_request.addrs.get(addr_idx) {
                    Some(addr) => {
                        tracing::trace!("the address {addr} is suspicious to the server, sending {num_bytes} bytes of data");
                        suspicious_addr.push(addr.clone());
                    }
                    None => {
                        return Err(Error::InvalidReferencedAddress {
                            index: addr_idx,
                            max: dial_request.addrs.len(),
                        });
                    }
                }

                println!("Time to bpay the tribute");
                send_aap_data(&mut coder, num_bytes).await?;
            }
            Response::Dial(dial_response) => {
                coder.close().await?;
                return test_end_from_dial_response(dial_request, dial_response, suspicious_addr);
            }
        }
    }
}

fn test_end_from_dial_response(
    req: DialRequest,
    resp: DialResponse,
    suspicious_addr: Vec<Multiaddr>,
) -> Result<TestEnd, Error> {
    if resp.addr_idx >= req.addrs.len() {
        return Err(Error::InvalidReferencedAddress {
            index: resp.addr_idx,
            max: req.addrs.len(),
        });
    }
    match (resp.status, resp.dial_status) {
        (ResponseStatus::E_REQUEST_REJECTED, _) => Err(Error::ServerRejectedDialRequest),
        (ResponseStatus::E_DIAL_REFUSED, _) => Err(Error::ServerChoseNotToDialAnyAddress),
        (ResponseStatus::E_INTERNAL_ERROR, _) => Err(Error::InternalServer),
        (ResponseStatus::OK, DialStatus::UNUSED) => Err(Error::InvalidResponse),
        (ResponseStatus::OK, DialStatus::E_DIAL_ERROR) => {
            Err(Error::UnableToConnectOnSelectedAddress {
                addr: req.addrs.get(resp.addr_idx).cloned(),
            })
        }
        (ResponseStatus::OK, DialStatus::E_DIAL_BACK_ERROR) => Err(Error::FailureDuringDialBack {
            addr: req.addrs.get(resp.addr_idx).cloned(),
        }),
        (ResponseStatus::OK, DialStatus::OK) => req
            .addrs
            .get(resp.addr_idx)
            .ok_or(Error::InvalidReferencedAddress {
                index: resp.addr_idx,
                max: req.addrs.len(),
            })
            .cloned()
            .map(|reachable_addr| TestEnd {
                dial_request: req,
                suspicious_addr,
                reachable_addr,
            }),
    }
}

async fn send_aap_data<I>(substream: &mut Coder<I>, num_bytes: usize) -> io::Result<()>
where
    I: AsyncWrite + Unpin,
{
    let count_full = num_bytes / DATA_FIELD_LEN_UPPER_BOUND;
    let partial_len = num_bytes % DATA_FIELD_LEN_UPPER_BOUND;
    for req in repeat(DATA_FIELD_LEN_UPPER_BOUND)
        .take(count_full)
        .chain(once(partial_len))
        .filter(|e| *e > 0)
        .map(|data_count| Request::Data(DialDataResponse { data_count }))
    {
        println!("Data req: {req:?}");
        substream.send_request(req).await?;
    }
    Ok(())
}
