use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    task::{Context, Poll},
};

use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    ConnectionDenied, ConnectionHandler, ConnectionId, FromSwarm, NetworkBehaviour, ToSwarm,
};

use crate::{
    client::{ResponseInfo, ToBehaviour},
    generated::structs::DialStatus,
    request_response::DialResponse,
};

use super::handler::Handler;

enum Command {
    TestListenerReachability {
        server_peer: PeerId,
        local_addrs: Vec<Multiaddr>,
    },
}

pub struct ReachabilityTestSucc {
    pub server_peer: PeerId,
    pub local_addr: Multiaddr,
}

#[derive(Debug, thiserror::Error)]
pub enum ReachabilityTestError {}

pub enum Event {
    CompletedReachabilityTest(Result<SuccessfullReachabilityTest, ReachabilityTestError>),
}

pub(super) struct Behaviour {
    pending_commands: VecDeque<Command>,
    accepted_nonce: Arc<scc::HashSet<u64>>,
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;

    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        Ok(Handler::new(self.accepted_nonce.clone(), peer))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<Self::ConnectionHandler, ConnectionDenied> {
        Ok(Handler::new(self.accepted_nonce.clone(), peer))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        todo!()
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: <Self::ConnectionHandler as ConnectionHandler>::ToBehaviour,
    ) {
        match event {
            ToBehaviour::ResponseInfo(ResponseInfo {
                response:
                    DialResponse {
                        status,
                        addr_idx,
                        dial_status,
                    },
                suspicious_addrs,
                successfull_addr,
            }) => {
                todo!()
            }
            _ => todo!(),
        }
        todo!()
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, <Self::ConnectionHandler as ConnectionHandler>::FromBehaviour>>
    {
        todo!()
    }
}
