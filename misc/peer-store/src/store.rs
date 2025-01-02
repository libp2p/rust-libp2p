use std::task::Context;

use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::FromSwarm;

/// A store that
/// - contains all observed addresses of peers;
pub trait Store {
    type Event;

    /// Update an address record.  
    /// Returns `true` when the address is new.  
    fn update_address(
        &mut self,
        peer: &PeerId,
        address: &Multiaddr,
        source: AddressSource,
        should_expire: bool,
    ) -> bool;

    /// Update an address record.  
    /// Returns `true` when the address is new.  
    fn update_certified_address(
        &mut self,
        signed_record: &libp2p_core::PeerRecord,
        source: AddressSource,
        should_expire: bool,
    ) -> bool;

    /// Remove an address record.
    /// Returns `true` when the address is removed.
    fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool;

    /// How this store handles events from the swarm.
    fn on_swarm_event(&mut self, event: &FromSwarm) -> Option<Event>;

    /// Get all stored addresses of the peer.
    fn addresses_of_peer(&self, peer: &PeerId) -> Option<impl Iterator<Item = &Multiaddr>>;

    /// Trigger grabage collection for records.
    fn poll(&mut self, cx: &mut Context<'_>) -> Option<Self::Event>;
}

pub enum Event {
    RecordUpdated(PeerId),
}

/// How the address is discovered.
#[derive(Debug, Clone, Copy)]
pub enum AddressSource {
    /// The address is discovered from a behaviour(e.g. kadelima, identify).
    Behaviour,
    /// We have direct connection to the address.
    DirectConnection,
    /// The address is manually added.
    Manual,
}
