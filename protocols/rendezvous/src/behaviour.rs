use crate::codec::{ErrorCode, Registration};
use crate::handler::RendezvousHandler;
use libp2p_core::connection::ConnectionId;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{
    IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
};
use std::collections::HashMap;
use std::task::{Context, Poll};

struct Rendezvous;

impl Rendezvous {
    pub fn register(&mut self, ns: String, rendezvous_node: PeerId) {}
    pub fn unregister(&mut self, ns: String, rendezvous_node: PeerId) {}
    pub fn discover(&mut self, ns: Option<String>, rendezvous_node: PeerId) {}
}

enum OutEvent {
    Discovered {
        rendezvous_node: PeerId,
        ns: Vec<Registration>,
    },
    FailedToDiscover {
        rendezvous_node: PeerId,
        err_code: ErrorCode,
    },
    RegisteredWithRendezvousNode {
        rendezvous_node: PeerId,
        ns: String,
    },
    FailedToRegisterWithRendezvousNode {
        rendezvous_node: PeerId,
        ns: String,
        err_code: ErrorCode,
    },
    DeclinedRegisterRequest {
        peer: PeerId,
        ns: String,
    },
    PeerRegistered {
        peer_id: PeerId,
        ns: Registration,
    },
    PeerUnregistered {
        peer_id: PeerId,
        ns: Registration,
    },
}

impl NetworkBehaviour for Rendezvous {
    type ProtocolsHandler = RendezvousHandler;
    type OutEvent = OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        RendezvousHandler::new()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        todo!()
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        todo!()
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        todo!()
    }

    fn inject_event(&mut self, peer_id: PeerId, connection: ConnectionId, event: _) {
        todo!()
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters<
            SupportedProtocolsIter = _,
            ListenedAddressesIter = _,
            ExternalAddressesIter = _,
        >,
    ) -> Poll<NetworkBehaviourAction<_, Self::OutEvent>> {
        todo!()
    }
}
