// two handlers, share state in behaviour
// do isolated stuff in async function
//
// write basic tests
// Take a look at rendezvous
// TODO: tests
// TODO: Handlers

mod dial_back;

use std::{
    cmp::min,
    collections::VecDeque,
    convert::identity,
    io,
    sync::Arc,
    task::Poll,
    time::Duration,
};

use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p_core::{upgrade::ReadyUpgrade, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    handler::{ConnectionEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound, ProtocolsChange},
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};


use crate::{
    client::ToBehaviour,
    generated::structs::mod_DialResponse::ResponseStatus,
    request_response::{
        DialBack, DialDataRequest, DialDataResponse, DialRequest, Request, Response,
    },
    DIAL_BACK_PROTOCOL_NAME, REQUEST_PROTOCOL_NAME, REQUEST_UPGRADE,
};
use crate::{DATA_FIELD_LEN_UPPER_BOUND, DATA_LEN_LOWER_BOUND, DATA_LEN_UPPER_BOUND};

use super::ResponseInfo;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_REQUESTS: usize = 10;

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("io error")]
    Io(#[from] io::Error),
    #[error("invalid data request index: {index} (max: {max})")]
    InvalidDataRequestIndex { index: usize, max: usize },
    #[error("data request too large: {len} (max: {max})")]
    DataRequestTooLarge { len: usize, max: usize },
    #[error("data request too small: {len} (min: {min})")]
    DataRequestTooSmall { len: usize, min: usize },
    #[error("timeout")]
    Timeout(#[from] futures_bounded::Timeout),
    #[error("invalid nonce: {nonce} (provided by (by))")]
    WrongNonceGiven { nonce: u64, by: PeerId },
    #[error("request protocol removed by peer")]
    RequestProtocolRemoved,
    #[error("dial back protocol removed by me")]
    DialBackProtocolRemoved,
}

pub(super) struct Handler {
    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
            <Self as ConnectionHandler>::Error,
        >,
    >,
    pending_data: VecDeque<PendingData>,
    queued_requests: VecDeque<DialRequest>,
    inbound: futures_bounded::FuturesSet<Result<(), Error>>,
    outbound: futures_bounded::FuturesSet<Result<PerformRequestStatus, Error>>,
    accepted_nonce: Arc<scc::HashSet<u64>>,
    remote_peer_id: PeerId,
}

impl Handler {
    pub(super) fn new(accepted_nonce: Arc<scc::HashSet<u64>>, remote_peer_id: PeerId) -> Self {
        Self {
            queued_events: VecDeque::new(),
            pending_data: VecDeque::new(),
            queued_requests: VecDeque::new(),
            inbound: futures_bounded::FuturesSet::new(DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS),
            outbound: futures_bounded::FuturesSet::new(DEFAULT_TIMEOUT, MAX_CONCURRENT_REQUESTS),
            accepted_nonce,
            remote_peer_id,
        }
    }
}

impl ConnectionHandler for Handler {
    type Error = Error;
    type ToBehaviour = super::ToBehaviour<Self::Error>;
    type FromBehaviour = super::FromBehaviour;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(crate::DIAL_BACK_UPGRADE, ())
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
            Self::Error,
        >,
    > {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        if let Some(pending_data) = self.pending_data.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(REQUEST_UPGRADE, Some(pending_data)),
            });
        }

        if let Poll::Ready(m) = self.outbound.poll_unpin(cx) {
            let perform_request_res = m.map_err(Error::Timeout).and_then(identity); // necessary until flatten_error stabilized
            match perform_request_res {
                Ok(PerformRequestStatus::Pending(pending)) => {
                    self.pending_data.push_back(pending);
                }
                Ok(PerformRequestStatus::Finished(resp_info)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        ToBehaviour::ResponseInfo(resp_info),
                    ))
                }
                Err(e) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        ToBehaviour::OutboundError(e),
                    ));
                }
            }
        }
        todo!()
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            super::FromBehaviour::Dial(dial_req) => {
                self.queued_requests.push_back(dial_req);
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
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol,
                info: None,
            }) => {
                let request = self
                    .queued_requests
                    .pop_front()
                    .expect("opened a stream without a penidng request");
                let request_clone = request.clone();
                if self
                    .outbound
                    .try_push(perform_dial_request(protocol, request))
                    .is_err()
                {
                    println!(
                        "Dropping outbound stream because we are at capacity. {request_clone:?}"
                    );
                    self.queued_requests.push_front(request_clone);
                }
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol,
                info: Some(pending_data),
            }) => {
                let pending_data_copy = pending_data.clone();
                if self
                    .outbound
                    .try_push(perform_data_request(protocol, pending_data))
                    .is_err()
                {
                    println!("Dropping outbound stream because we are at capacity.");
                    self.pending_data.push_front(pending_data_copy);
                }
            }
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol, ..
            }) => {
                let remote_peer_id = self.remote_peer_id.clone();
                if self
                    .inbound
                    .try_push(perform_dial_back(
                        protocol,
                        self.accepted_nonce.clone(),
                        remote_peer_id,
                    ))
                    .is_err()
                {
                    println!("Dropping inbound stream because we are at capacity.");
                }
            }
            ConnectionEvent::DialUpgradeError(_err) => {
                todo!()
            }
            ConnectionEvent::ListenUpgradeError(_err) => {
                todo!()
            }
            ConnectionEvent::AddressChange(_) => {}
            ConnectionEvent::LocalProtocolsChange(ProtocolsChange::Removed(mut protocols)) => {
                if protocols.any(|e| &DIAL_BACK_PROTOCOL_NAME == e) {
                    self.queued_events.push_back(ConnectionHandlerEvent::Close(
                        Error::DialBackProtocolRemoved,
                    ));
                }
            }
            ConnectionEvent::RemoteProtocolsChange(ProtocolsChange::Removed(mut protocols)) => {
                if protocols.any(|e| &REQUEST_PROTOCOL_NAME == e) {
                    self.queued_events
                        .push_back(ConnectionHandlerEvent::Close(Error::RequestProtocolRemoved));
                }
            }
            _ => {}
        }
        todo!()
    }
}

enum PerformRequestStatus {
    Finished(ResponseInfo),
    Pending(PendingData),
}

#[derive(Debug, Clone)]
pub(super) struct PendingData {
    data_count: usize,
    suspicious_addrs: Vec<Multiaddr>,
    all_addrs: Vec<Multiaddr>,
}

async fn perform_dial_request<S>(
    mut stream: S,
    dial_req: DialRequest,
) -> Result<PerformRequestStatus, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let addrs = dial_req.addrs.clone();
    let req = Request::Dial(dial_req);
    req.write_into(&mut stream).await?;
    let resp = Response::read_from(&mut stream).await?;
    stream.close().await?;
    let DialDataRequest {
        addr_idx,
        num_bytes,
    } = match resp {
        Response::Dial(resp) => {
            let successfull_addr = if resp.status == ResponseStatus::OK {
                addrs.get(resp.addr_idx).cloned()
            } else {
                None
            };
            return Ok(PerformRequestStatus::Finished(ResponseInfo::new(
                resp,
                Vec::new(),
                successfull_addr,
            )));
        }
        Response::Data(data_req) => data_req,
    };
    if num_bytes > DATA_LEN_UPPER_BOUND {
        return Err(Error::DataRequestTooLarge {
            len: num_bytes,
            max: DATA_LEN_UPPER_BOUND,
        });
    } else if num_bytes < DATA_LEN_LOWER_BOUND {
        return Err(Error::DataRequestTooSmall {
            len: num_bytes,
            min: DATA_LEN_LOWER_BOUND,
        });
    }
    let suspicious_addr = addrs
        .get(addr_idx)
        .ok_or(Error::InvalidDataRequestIndex {
            index: addr_idx,
            max: addrs.len(),
        })?
        .clone();
    Ok(PerformRequestStatus::Pending(PendingData {
        data_count: num_bytes,
        suspicious_addrs: vec![suspicious_addr],
        all_addrs: addrs,
    }))
}

async fn perform_data_request<S>(
    mut stream: S,
    PendingData {
        mut data_count,
        mut suspicious_addrs,
        all_addrs,
    }: PendingData,
) -> Result<PerformRequestStatus, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let data_field_len = min(DATA_FIELD_LEN_UPPER_BOUND, data_count);
    data_count -= data_field_len;
    let req = Request::Data(DialDataResponse {
        data_count: data_field_len,
    });
    req.write_into(&mut stream).await?;
    if data_count != 0 {
        return Ok(PerformRequestStatus::Pending(PendingData {
            data_count,
            suspicious_addrs,
            all_addrs,
        }));
    }
    let resp = Response::read_from(&mut stream).await?;
    stream.close().await?;
    match resp {
        Response::Dial(dial_resp) => {
            let successfull_addr = if dial_resp.status == ResponseStatus::OK {
                all_addrs.get(dial_resp.addr_idx).cloned()
            } else {
                None
            };
            Ok(PerformRequestStatus::Finished(ResponseInfo::new(
                dial_resp,
                suspicious_addrs,
                successfull_addr,
            )))
        }
        Response::Data(DialDataRequest {
            addr_idx,
            num_bytes,
        }) => {
            if num_bytes > DATA_LEN_UPPER_BOUND {
                return Err(Error::DataRequestTooLarge {
                    len: num_bytes,
                    max: DATA_LEN_UPPER_BOUND,
                });
            } else if num_bytes < DATA_LEN_LOWER_BOUND {
                return Err(Error::DataRequestTooSmall {
                    len: num_bytes,
                    min: DATA_LEN_LOWER_BOUND,
                });
            }
            let suspicious_addr =
                all_addrs
                    .get(addr_idx)
                    .ok_or(Error::InvalidDataRequestIndex {
                        index: addr_idx,
                        max: all_addrs.len(),
                    })?;
            if !suspicious_addrs.contains(suspicious_addr) {
                suspicious_addrs.push(suspicious_addr.clone());
            }
            Ok(PerformRequestStatus::Pending(PendingData {
                data_count: num_bytes,
                suspicious_addrs,
                all_addrs,
            }))
        }
    }
}

async fn perform_dial_back<S>(
    mut stream: S,
    accepted_nonce: Arc<scc::HashSet<u64>>,
    remote_peer_id: PeerId,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let DialBack { nonce } = DialBack::read_from(&mut stream).await?;
    if !accepted_nonce.contains_async(&nonce).await {
        return Err(Error::WrongNonceGiven {
            nonce,
            by: remote_peer_id,
        });
    }
    stream.close().await?;
    Ok(())
}
