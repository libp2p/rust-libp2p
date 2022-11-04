use crate::behaviour::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use crate::handler::{
    ConnectionHandlerEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound, KeepAlive,
    StreamEvent, SubstreamProtocol,
};
use libp2p_core::connection::ConnectionId;
use libp2p_core::upgrade::DeniedUpgrade;
use libp2p_core::PeerId;
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

    fn on_behaviour_event(&mut self, v: Self::InEvent) {
        void::unreachable(v)
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

    fn on_event(
        &mut self,
        event: StreamEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            StreamEvent::FullyNegotiatedInbound(FullyNegotiatedInbound { protocol, .. }) => {
                void::unreachable(protocol)
            }
            StreamEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound { protocol, .. }) => {
                void::unreachable(protocol)
            }
            StreamEvent::DialUpgradeError(_)
            | StreamEvent::ListenUpgradeError(_)
            | StreamEvent::AddressChange(_) => {}
        }
    }
}
