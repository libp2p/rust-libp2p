use crate::behaviour::THandlerInEvent;
use crate::{dummy, CloseConnection, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{ConnectedPoint, PeerId};
use std::collections::{HashSet, VecDeque};
use std::task::{Context, Poll};

#[derive(Default)]
pub struct Behaviour {
    list: HashSet<PeerId>,
    to_disconnect: VecDeque<PeerId>,
}

impl Behaviour {
    pub fn add_peer(&mut self, peer: PeerId) {
        self.list.insert(peer);
        self.to_disconnect.push_back(peer);
    }

    pub fn remove_peer(&mut self, peer: PeerId) {
        self.list.remove(&peer);
    }
}

#[derive(Debug, thiserror::Error)]
#[error("peer is on a deny list")]
pub struct Denied;

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ConnectionDenied = Denied;
    type OutEvent = ();

    fn new_handler(
        &mut self,
        peer: &PeerId,
        _: &ConnectedPoint,
    ) -> Result<Self::ConnectionHandler, Self::ConnectionDenied> {
        if self.list.contains(peer) {
            return Err(Denied);
        }

        Ok(dummy::ConnectionHandler)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self::ConnectionHandler>>>
    {
        if let Some(peer_id) = self.to_disconnect.pop_front() {
            return Poll::Ready(NetworkBehaviourAction::CloseConnection {
                peer_id,
                connection: CloseConnection::All,
            });
        }

        Poll::Pending
    }
}
