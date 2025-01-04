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

#[async_std::test]
async fn sends_remaining_events_to_behaviour_on_connection_close() {
    let mut swarm1 = Swarm::new_ephemeral(|_| Behaviour::new(3));
    let mut swarm2 = Swarm::new_ephemeral(|_| Behaviour::new(3));

    swarm2.listen().with_memory_addr_external().await;
    swarm1.connect(&mut swarm2).await;

    swarm1.disconnect_peer_id(*swarm2.local_peer_id()).unwrap();

    match libp2p_swarm_test::drive(&mut swarm1, &mut swarm2).await {
        ([SwarmEvent::ConnectionClosed { .. }], [SwarmEvent::ConnectionClosed { .. }]) => {
            assert_eq!(swarm1.behaviour().state, 0);
            assert_eq!(swarm2.behaviour().state, 0);
        }
        (e1, e2) => panic!("Unexpected events: {:?} {:?}", e1, e2),
    }
}

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

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn connection_keep_alive(&self) -> bool {
        true
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
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
        _: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
    }
}
