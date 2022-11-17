use crate::behaviour::{FromSwarm, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use crate::handler::{
    ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive, SubstreamProtocol,
};
use crate::NegotiatedSubstream;
use libp2p_core::connection::ConnectionId;
use libp2p_core::PeerId;
use libp2p_core::{
    upgrade::{DeniedUpgrade, InboundUpgrade, OutboundUpgrade},
    Multiaddr,
};
use std::task::{Context, Poll};
use void::Void;

/// Implementation of [`NetworkBehaviour`] that doesn't do anything other than keep all connections alive.
///
/// This is primarily useful for test code. In can however occasionally be useful for production code too.
/// The caveat is that open connections consume system resources and should thus be shutdown when
/// they are not in use. Connections can also fail at any time so really, your application should be
/// designed to establish them when necessary, making the use of this behaviour likely redundant.
#[derive(Default)]
pub struct Behaviour;

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = ConnectionHandler;
    type OutEvent = Void;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        ConnectionHandler
    }

    fn on_connection_handler_event(&mut self, _: PeerId, _: ConnectionId, event: Void) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(_)
            | FromSwarm::ConnectionClosed(_)
            | FromSwarm::AddressChange(_)
            | FromSwarm::DialFailure(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListener(_)
            | FromSwarm::NewListenAddr(_)
            | FromSwarm::ExpiredListenAddr(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddr(_)
            | FromSwarm::ExpiredExternalAddr(_) => {}
        }
    }
}

/// Implementation of [`ConnectionHandler`] that doesn't handle anything but keeps the connection alive.
#[derive(Clone, Debug)]
pub struct ConnectionHandler;

impl crate::handler::ConnectionHandler for ConnectionHandler {
    type InEvent = Void;
    type OutEvent = Void;
    type Error = Void;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = Void;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
        void::unreachable(protocol);
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        void::unreachable(protocol)
    }

    fn inject_event(&mut self, v: Self::InEvent) {
        void::unreachable(v)
    }

    fn inject_address_change(&mut self, _: &Multiaddr) {}

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ConnectionHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
    }

    fn inject_listen_upgrade_error(
        &mut self,
        _: Self::InboundOpenInfo,
        _: ConnectionHandlerUpgrErr<
            <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        Poll::Pending
    }
}
