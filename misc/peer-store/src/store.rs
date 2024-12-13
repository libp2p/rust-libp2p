use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::FromSwarm;

/// A store that
/// - keep track of currently connected peers;
/// - contains all observed addresses of peers;
pub trait Store {
    /// Update an address record.  
    /// Returns `true` when the address is new.  
    fn update_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool;
    /// Remove an address record.
    /// Returns `true` when the address is removed.
    fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool;
    /// How this store handles events from the swarm.
    fn on_swarm_event(&mut self, event: &FromSwarm) -> Option<Event>;
    /// Get all stored addresses of the peer.
    fn addresses_of_peer(
        &self,
        peer: &PeerId,
    ) -> Option<impl Iterator<Item = super::AddressRecord>>;
}

pub enum Event {
    RecordUpdated(PeerId),
}
