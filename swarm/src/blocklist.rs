use crate::handler::TryIntoConnectionHandler;
use crate::{dummy, ConnectionHandler, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::connection::ConnectionId;
use libp2p_core::{ConnectedPoint, PeerId};
use std::collections::HashSet;
use std::fmt;
use std::task::{Context, Poll};
use void::Void;

/// A [`NetworkBehaviour`] that blocks connections to a set of specified peers.
pub struct Behaviour {
    blocked_peers: HashSet<PeerId>,
}

impl Behaviour {
    pub fn block_peer(&mut self, peer: PeerId) {
        self.blocked_peers.insert(peer);
    }

    pub fn unblock_peer(&mut self, peer: &PeerId) {
        self.blocked_peers.remove(peer);
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = ConnectionHandlerProto;
    type OutEvent = Void;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        ConnectionHandlerProto {
            blocked_peers: self.blocked_peers.clone(),
        }
    }

    fn inject_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        never: <<Self::ConnectionHandler as TryIntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
    ) {
        void::unreachable(never)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        Poll::Pending
    }
}

pub struct ConnectionHandlerProto {
    blocked_peers: HashSet<PeerId>,
}

impl TryIntoConnectionHandler for ConnectionHandlerProto {
    type Handler = dummy::ConnectionHandler;
    type Error = Blocked;

    fn try_into_handler(
        self,
        remote_peer_id: &PeerId,
        _: &ConnectedPoint,
    ) -> Result<Self::Handler, Self::Error> {
        if self.blocked_peers.contains(remote_peer_id) {
            return Err(Blocked {
                peer: *remote_peer_id,
            });
        }

        Ok(dummy::ConnectionHandler)
    }

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
        todo!()
    }
}

#[derive(Debug)]
pub struct Blocked {
    pub peer: PeerId,
}

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer {} is blocked", self.peer)
    }
}

impl std::error::Error for Blocked {}
