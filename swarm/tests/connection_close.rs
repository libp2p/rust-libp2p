use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use libp2p_core::{transport::PortUse, upgrade::DeniedUpgrade, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    handler::ConnectionEvent, ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent,
    ConnectionId, FromSwarm, NetworkBehaviour, SubstreamProtocol, Swarm, SwarmEvent, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p_swarm_test::SwarmExt;



struct HandlerWithState {
    precious_state: u64,
}

struct Behaviour {
    state: u64,
}

impl Behaviour {
    fn new(state: u64) -> Self {
        Behaviour { state }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = HandlerWithState;
    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(HandlerWithState {
            precious_state: self.state,
        })
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(HandlerWithState {
            precious_state: self.state,
        })
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        if let FromSwarm::ConnectionClosed(_) = event {
            assert_eq!(self.state, 0);
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        assert_eq!(self.state, event);
        self.state -= 1;
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

impl ConnectionHandler for HandlerWithState {
    type FromBehaviour = Infallible;
    type ToBehaviour = u64;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn connection_keep_alive(&self) -> bool {
        true
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        Poll::Pending
    }

    fn poll_close(&mut self, _: &mut Context<'_>) -> Poll<Option<Self::ToBehaviour>> {
        if self.precious_state > 0 {
            let state = self.precious_state;
            self.precious_state -= 1;

            return Poll::Ready(Some(state));
        }

        Poll::Ready(None)
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        libp2p_core::util::unreachable(event)
    }

    fn on_connection_event(
        &mut self,
        _: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
    }
}
