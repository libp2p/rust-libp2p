use crate::behaviour::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use crate::handler::ConnectionHandler;
use crate::handler::DummyConnectionHandler;
use libp2p_core::connection::ConnectionId;
use libp2p_core::PeerId;
use std::task::{Context, Poll};

/// Implementation of [`NetworkBehaviour`] that doesn't do anything other than keep all connections alive.
pub struct Dummy;

impl NetworkBehaviour for Dummy {
    type ConnectionHandler = DummyConnectionHandler;
    type OutEvent = void::Void;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        DummyConnectionHandler
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
