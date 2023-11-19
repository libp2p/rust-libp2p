use futures::{AsyncRead, AsyncWrite};
use futures_bounded::FuturesSet;
use libp2p_core::{
    upgrade::{DeniedUpgrade, ReadyUpgrade},
    Multiaddr,
};

use libp2p_swarm::{
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedOutbound, ProtocolsChange},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use scc::hash_cache::DEFAULT_MAXIMUM_CAPACITY;
use std::{
    collections::VecDeque,
    convert::identity,
    io,
    iter::{once, repeat},
    task::{Context, Poll},
};

use crate::{
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    request_response::{
        DialDataRequest, DialDataResponse, DialRequest, DialResponse, Request, Response,
        DATA_FIELD_LEN_UPPER_BOUND, DATA_LEN_LOWER_BOUND, DATA_LEN_UPPER_BOUND,
    },
    REQUEST_PROTOCOL_NAME, REQUEST_UPGRADE,
};

use super::DEFAULT_TIMEOUT;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
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
    InternalServerError,
    #[error("server did not respond correctly to dial request")]
    InvalidResponse,
    #[error("server was unable to connect to address: {addr:?}")]
    UnableToConnectOnSelectedAddress { addr: Option<Multiaddr> },
    #[error("server experienced failure during dial back on address: {addr:?}")]
    FailureDuringDialBack { addr: Option<Multiaddr> },
}

#[derive(Debug)]
pub(crate) struct TestEnd {
    pub(crate) dial_request: DialRequest,
    pub(crate) suspicious_addr: Vec<Multiaddr>,
    pub(crate) reachable_addr: Multiaddr,
}

#[derive(Debug)]
pub(crate) enum ToBehaviour {
    TestCompleted(Result<TestEnd, Error>),
    PeerHasServerSupport,
}

#[derive(Debug)]
pub(crate) enum FromBehaviour {
    PerformRequest(DialRequest),
}

pub(crate) struct Handler {
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,
    outbound: futures_bounded::FuturesSet<Result<TestEnd, Error>>,
    queued_requests: VecDeque<DialRequest>,
}

impl Handler {
    pub(crate) fn new() -> Self {
        Self {
            queued_events: VecDeque::new(),
            outbound: FuturesSet::new(DEFAULT_TIMEOUT, DEFAULT_MAXIMUM_CAPACITY),
            queued_requests: VecDeque::new(),
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
        if !self.queued_requests.is_empty() {
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(REQUEST_UPGRADE, ()),
            });
        }
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            FromBehaviour::PerformRequest(req) => {
                self.queued_requests.push_back(req);
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
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => match self.queued_requests.pop_front() {
                Some(dial_req) => {
                    if self
                        .outbound
                        .try_push(handle_substream(dial_req.clone(), protocol))
                        .is_err()
                    {
                        tracing::warn!("Dial request dropped, too many requests in flight");
                        self.queued_requests.push_front(dial_req);
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

async fn handle_substream(
    dial_request: DialRequest,
    mut substream: impl AsyncRead + AsyncWrite + Unpin,
) -> Result<TestEnd, Error> {
    Request::Dial(dial_request.clone())
        .write_into(&mut substream)
        .await?;
    let mut suspicious_addr = Vec::new();
    loop {
        match Response::read_from(&mut substream).await? {
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

                send_aap_data(&mut substream, num_bytes).await?;
            }
            Response::Dial(dial_response) => {
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
        (ResponseStatus::E_INTERNAL_ERROR, _) => Err(Error::InternalServerError),
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

async fn send_aap_data(mut substream: impl AsyncWrite + Unpin, num_bytes: usize) -> io::Result<()> {
    let count_full = num_bytes / DATA_FIELD_LEN_UPPER_BOUND;
    let partial_len = num_bytes % DATA_FIELD_LEN_UPPER_BOUND;
    for req in repeat(DATA_FIELD_LEN_UPPER_BOUND)
        .take(count_full)
        .chain(once(partial_len))
        .filter(|e| *e > 0)
        .map(|data_count| Request::Data(DialDataResponse { data_count }))
    {
        req.write_into(&mut substream).await?;
    }
    Ok(())
}
