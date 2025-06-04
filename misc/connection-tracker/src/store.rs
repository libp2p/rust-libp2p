use libp2p_core::PeerId;
use libp2p_swarm::ConnectionId;
use std::collections::{HashMap, HashSet};

/// Simple storage for connected peers.
#[derive(Debug, Default)]
pub struct ConnectionStore {
    /// Currently connected peers with their connection IDs
    connected: HashMap<PeerId, HashSet<ConnectionId>>,
}

impl ConnectionStore {
    /// Create a new connection store.
    pub fn new() -> Self {
        Self {
            connected: HashMap::new(),
        }
    }

    /// Add a new connection for a peer.
    /// Returns `true` if this is the first connection to the peer.
    pub fn connection_established(&mut self, peer_id: PeerId, connection_id: ConnectionId) -> bool {
        let connections = self.connected.entry(peer_id).or_insert_with(HashSet::new);
        let is_first_connection = connections.is_empty();
        connections.insert(connection_id);
        is_first_connection
    }

    /// Remove a connection for a peer.
    /// Returns `true` if this was the last connection to the peer.
    pub fn connection_closed(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        remaining_established: usize,
    ) -> bool {
        if let Some(connections) = self.connected.get_mut(&peer_id) {
            connections.remove(&connection_id);

            if remaining_established == 0 {
                self.connected.remove(&peer_id);
                return true;
            }
        }
        false
    }

    /// Check if a peer is currently connected.
    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.connected.contains_key(peer_id)
    }

    /// Get all connected peer IDs.
    pub fn connected_peers(&self) -> impl Iterator<Item = &PeerId> {
        self.connected.keys()
    }

    /// Get the number of connected peers.
    pub fn connected_count(&self) -> usize {
        self.connected.len()
    }

    /// Get the number of connections to a specific peer.
    pub fn connection_count(&self, peer_id: &PeerId) -> usize {
        self.connected
            .get(peer_id)
            .map(|connections| connections.len())
            .unwrap_or(0)
    }
}
