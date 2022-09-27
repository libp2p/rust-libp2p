use crate::behaviour::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use crate::handler::ConnectionHandler;
use crate::handler::KeepAliveConnectionHandler;
use libp2p_core::connection::ConnectionId;
use libp2p_core::PeerId;
use std::task::{Context, Poll};

/// Implementation of [`NetworkBehaviour`] that doesn't do anything other than keep all connections alive.
///
/// This is primarily useful for test code. In can however occasionally be useful for production code too.
/// The caveat is that open connections consume system resources and should thus be shutdown when
/// they are not in use. Connections can also fail at any time so really, your application should be
/// designed to establish them when necessary, making the use of this behaviour likely redundant.
#[derive(Default)]
pub struct Behaviour;

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = KeepAliveConnectionHandler;
    type OutEvent = void::Void;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        KeepAliveConnectionHandler
    }

    fn inject_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        event: <Self::ConnectionHandler as ConnectionHandler>::OutEvent,
    ) {
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
