use crate::behaviour::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use crate::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
};
use crate::{ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive, SubstreamProtocol};
use libp2p_core::connection::ConnectionId;
use libp2p_core::upgrade::DeniedUpgrade;
use libp2p_core::PeerId;
use libp2p_core::UpgradeError;
use std::task::{Context, Poll};
use void::Void;

/// Implementation of [`NetworkBehaviour`] that doesn't do anything.
pub struct Behaviour;

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = ConnectionHandler;
    type OutEvent = Void;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        ConnectionHandler
    }

    fn inject_event(&mut self, _: PeerId, _: ConnectionId, event: Void) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        Poll::Pending
    }
}

/// An implementation of [`ConnectionHandler`] that neither handles any protocols nor does it keep the connection alive.
#[derive(Clone)]
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

    fn on_behaviour_event(&mut self, event: Self::InEvent) {
        void::unreachable(event)
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::No
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
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol, ..
            }) => void::unreachable(protocol),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => void::unreachable(protocol),
            ConnectionEvent::DialUpgradeError(DialUpgradeError { info: _, error }) => match error {
                ConnectionHandlerUpgrErr::Timeout => unreachable!(),
                ConnectionHandlerUpgrErr::Timer => unreachable!(),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)) => void::unreachable(e),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(_)) => {
                    unreachable!("Denied upgrade does not support any protocols")
                }
            },
            ConnectionEvent::AddressChange(_) | ConnectionEvent::ListenUpgradeError(_) => {}
        }
    }
}
