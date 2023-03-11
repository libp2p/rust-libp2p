use crate::derive_prelude::Multiaddr;
use crate::{
    dummy, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, NetworkBehaviourAction,
    PollParameters, THandler, THandlerInEvent, THandlerOutEvent,
};
use libp2p_core::{Endpoint, PeerId};
use std::collections::HashSet;
use std::task::{Context, Poll};
use void::Void;

#[derive(Default, Debug)]
pub struct Behaviour {
    blocked_peers: HashSet<PeerId>,
}

impl Behaviour {
    pub fn block(&mut self, peer: PeerId) {
        self.blocked_peers.insert(peer);
    }

    pub fn unblock(&mut self, peer: &PeerId) {
        self.blocked_peers.remove(peer);
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type OutEvent = Void;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        todo!()
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        todo!()
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        todo!()
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        todo!()
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Swarm;
    use libp2p_swarm_test::SwarmExt as _;

    #[test]
    fn cannot_dial_blocked_peer() {
        let mut swarm1 = Swarm::new_ephemeral(|_| Behaviour::default());
        let mut swarm2 = Swarm::new_ephemeral(|_| Behaviour::default());

        swarm1.behaviour_mut().block(swarm2.local_peer_id());
    }

    #[test]
    fn disconnect_peer_upon_blocking() {
        todo!()
    }

    #[test]
    fn blocked_peer_cannot_connect() {
        todo!()
    }
}
