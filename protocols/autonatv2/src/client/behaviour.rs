use std::{
    collections::{VecDeque},
    sync::Arc,
    task::{Context, Poll},
};

use ip_global::IpExt;
use libp2p_core::{multiaddr::Protocol, transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dial_opts::DialOpts, ConnectionDenied, ConnectionHandler, ConnectionId, FromSwarm,
    NetworkBehaviour, NotifyHandler, ToSwarm,
};
use rand_core::RngCore;

use crate::{
    client::{ResponseInfo, ToBehaviour},
    generated::structs::{mod_DialResponse::ResponseStatus, DialStatus},
    request_response::{DialRequest, DialResponse},
};

use super::{handler::Handler, FromBehaviour};

enum Command {
    TestListenerReachability {
        server_peer: PeerId,
        local_addrs: Vec<Multiaddr>,
    },
}

pub(crate) struct ReachabilityTestSucc {
    pub(crate) server_peer: PeerId,
    pub(crate) visible_addr: Option<Multiaddr>,
    pub(crate) suspicious_addrs: Vec<Multiaddr>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReachabilityTestErr {
    #[error("Server chose not to dial any provided address. Server peer id: {peer_id}")]
    ServerChoseNotToDialAnyAddress { peer_id: PeerId },
    #[error("Server rejected dial request. Server peer id: {peer_id}")]
    ServerRejectedDialRequest { peer_id: PeerId },
    #[error("Server ran into an internal error. Server peer id: {peer_id}")]
    InternalServerError { peer_id: PeerId },
    #[error("Server did not respond correctly to dial request. Server peer id: {peer_id}")]
    InvalidResponse { peer_id: PeerId },
    #[error("Server was unable to connect to address: {addr:?}. Server peer id: {peer_id}")]
    UnableToConnectOnSelectedAddress {
        peer_id: PeerId,
        addr: Option<Multiaddr>,
    },
    #[error("Server experienced failure during dial back on address: {addr:?} Server peer id: {peer_id}")]
    FailureDuringDialBack {
        peer_id: PeerId,
        addr: Option<Multiaddr>,
    },
}

pub(crate) enum Event {
    CompletedReachabilityTest(Result<ReachabilityTestSucc, ReachabilityTestErr>),
}

pub(super) struct Behaviour<R>
where
    R: RngCore + 'static,
{
    pending_commands: VecDeque<Command>,
    pending_events: VecDeque<
        ToSwarm<
            <Self as NetworkBehaviour>::ToSwarm,
            <<Self as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::FromBehaviour,
        >,
    >,
    accepted_nonce: Arc<scc::HashSet<u64>>,
    rng: R,
}

impl<R> NetworkBehaviour for Behaviour<R>
where
    R: RngCore + 'static,
{
    type ConnectionHandler = Handler;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        Ok(Handler::new(self.accepted_nonce.clone(), peer))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
        _port_use: PortUse,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        Ok(Handler::new(self.accepted_nonce.clone(), peer))
    }

    fn on_swarm_event(&mut self, _event: FromSwarm) {
        todo!()
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: ConnectionId,
        event: <Self::ConnectionHandler as ConnectionHandler>::ToBehaviour,
    ) {
        match event {
            ToBehaviour::ResponseInfo(response_info) => {
                let event = Event::CompletedReachabilityTest(
                    self.handle_response_info(peer_id, response_info),
                );
                self.pending_events.push_back(ToSwarm::GenerateEvent(event));
            }
            _ => todo!(),
        }
        todo!()
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, <Self::ConnectionHandler as ConnectionHandler>::FromBehaviour>>
    {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        if let Some(command) = self.pending_commands.pop_front() {
            self.handle_command(command);
            return Poll::Pending;
        }
        todo!()
    }
}

impl<R> Behaviour<R>
where
    R: RngCore,
{
    fn handle_response_info(
        &mut self,
        peer_id: PeerId,
        ResponseInfo {
            response:
                DialResponse {
                    status,
                    dial_status,
                    ..
                },
            suspicious_addrs,
            successfull_addr,
        }: ResponseInfo,
    ) -> Result<ReachabilityTestSucc, ReachabilityTestErr> {
        match (status, dial_status) {
            (ResponseStatus::E_REQUEST_REJECTED, _) => {
                Err(ReachabilityTestErr::ServerRejectedDialRequest { peer_id })
            }
            (ResponseStatus::E_DIAL_REFUSED, _) => {
                Err(ReachabilityTestErr::ServerChoseNotToDialAnyAddress { peer_id })
            }
            (ResponseStatus::E_INTERNAL_ERROR, _) => {
                Err(ReachabilityTestErr::InternalServerError { peer_id })
            }
            (ResponseStatus::OK, DialStatus::UNUSED) => {
                Err(ReachabilityTestErr::InvalidResponse { peer_id })
            }
            (ResponseStatus::OK, DialStatus::E_DIAL_ERROR) => {
                Err(ReachabilityTestErr::UnableToConnectOnSelectedAddress {
                    peer_id,
                    addr: successfull_addr,
                })
            }
            (ResponseStatus::OK, DialStatus::E_DIAL_BACK_ERROR) => {
                Err(ReachabilityTestErr::FailureDuringDialBack {
                    peer_id,
                    addr: successfull_addr,
                })
            }
            (ResponseStatus::OK, DialStatus::OK) => Ok(ReachabilityTestSucc {
                server_peer: peer_id,
                visible_addr: successfull_addr,
                suspicious_addrs,
            }),
        }
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::TestListenerReachability {
                server_peer,
                local_addrs,
            } => {
                let _cleaned_local_addrs = local_addrs.iter().filter(|addr| {
                    !addr.iter().any(|p| match p {
                        Protocol::Ip4(ip) if !IpExt::is_global(&ip) => true,
                        Protocol::Ip6(ip) if !IpExt::is_global(&ip) => true,
                        Protocol::Dns(m)
                        | Protocol::Dns4(m)
                        | Protocol::Dns6(m)
                        | Protocol::Dnsaddr(m) => m == "localhost" || m.ends_with(".local"),
                        _ => false,
                    })
                });
                let dial_opts = DialOpts::peer_id(server_peer.clone()).build();
                let dial_event = ToSwarm::Dial { opts: dial_opts };
                self.pending_events.push_back(dial_event);
                let dial_request = DialRequest {
                    addrs: local_addrs,
                    nonce: self.rng.next_u64(),
                };
                self.pending_events.push_back(ToSwarm::NotifyHandler {
                    peer_id: server_peer,
                    handler: NotifyHandler::Any,
                    event: FromBehaviour::Dial(dial_request),
                })
            }
        }
    }
}
