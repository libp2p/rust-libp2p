use std::{collections::HashMap, time::SystemTime};

use libp2p_core::{Multiaddr, PeerId};

/// A store that
/// - keep track of currently connected peers;
/// - contains all observed addresses of peers;
pub trait Store {
    /// Update an address record.  
    /// Return `true` when the address is new.  
    fn on_address_update(&mut self, peer: &PeerId, address: &Multiaddr) -> bool;
    fn addresses_of_peer(
        &self,
        peer: &PeerId,
    ) -> Option<impl Iterator<Item = super::AddressRecord>>;
}

pub(crate) struct PeerAddressRecord {
    addresses: HashMap<Multiaddr, AddressRecord>,
}
impl PeerAddressRecord {
    pub(crate) fn records(&self) -> impl Iterator<Item = super::AddressRecord> {
        self.addresses
            .iter()
            .map(|(address, record)| super::AddressRecord {
                address,
                last_seen: &record.last_seen,
            })
    }
    pub(crate) fn new(address: &Multiaddr) -> Self {
        let mut address_book = HashMap::new();
        address_book.insert(address.clone(), AddressRecord::new());
        Self {
            addresses: address_book,
        }
    }
    pub(crate) fn on_address_update(&mut self, address: &Multiaddr) -> bool {
        if let Some(record) = self.addresses.get_mut(address) {
            record.update_last_seen();
            false
        } else {
            self.addresses.insert(address.clone(), AddressRecord::new());
            true
        }
    }
}

pub(crate) struct AddressRecord {
    /// The time when the address is last seen.
    last_seen: SystemTime,
}
impl AddressRecord {
    pub(crate) fn new() -> Self {
        Self {
            last_seen: SystemTime::now(),
        }
    }
    pub(crate) fn update_last_seen(&mut self) {
        self.last_seen = SystemTime::now();
    }
}

/// A in-memory store.
pub struct MemoryStore {
    /// An address book of peers regardless of their status(connected or not).
    address_book: HashMap<PeerId, PeerAddressRecord>,
}

impl Store for MemoryStore {
    fn on_address_update(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        if let Some(record) = self.address_book.get_mut(peer) {
            record.on_address_update(address)
        } else {
            self.address_book
                .insert(*peer, PeerAddressRecord::new(address));
            true
        }
    }
    fn addresses_of_peer(
        &self,
        peer: &PeerId,
    ) -> Option<impl Iterator<Item = super::AddressRecord>> {
        self.address_book.get(peer).map(|record| record.records())
    }
}
