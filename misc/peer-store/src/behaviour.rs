use libp2p_swarm::{dummy, NetworkBehaviour};

use crate::store::Store;

pub enum Event{
    RecordUpdated
}

pub struct Behaviour<S> {
    store: S,
}

impl<S> NetworkBehaviour for Behaviour<S>
where
    S: Store + Send + Sync + 'static,
{
    type ConnectionHandler = dummy::ConnectionHandler;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        _peer: libp2p_core::PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        _peer: libp2p_core::PeerId,
        _addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        todo!()
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p_core::PeerId,
        _connection_id: libp2p_swarm::ConnectionId,
        _event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        todo!()
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        todo!()
    }
}
