use std::{
    collections::{HashMap, HashSet},
    time::SystemTime,
};

use libp2p_core::{Multiaddr, PeerId};

pub trait Store {
    fn on_peer_connect(&mut self, peer: &PeerId);
    fn on_peer_disconnect(&mut self, peer: &PeerId);
    /// Update an address record.  
    /// Return `true` when the address is new.  
    fn on_address_update(&mut self, peer: &PeerId, address: &Multiaddr) -> bool;
    fn list_connected(&self) -> Box<[&PeerId]>;
    fn addresses_of_peer(&self, peer: &PeerId) -> Option<Box<[super::AddressRecord]>>;
}

pub(crate) struct PeerAddressRecord {
    addresses: HashMap<Multiaddr, AddressRecord>,
}
impl PeerAddressRecord {
    pub(crate) fn records(&self) -> Box<[super::AddressRecord]> {
        self.addresses
            .iter()
            .map(|(address, record)| super::AddressRecord {
                address,
                last_seen: &record.last_seen,
            })
            .collect()
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
    /// The time when the address is last seen, in seconds.
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

pub struct MemoryStore {
    connected_peers: HashSet<PeerId>,
    address_book: HashMap<PeerId, PeerAddressRecord>,
}

impl Store for MemoryStore {
    fn on_peer_connect(&mut self, peer: &PeerId) {
        self.connected_peers.insert(*peer);
    }
    fn on_peer_disconnect(&mut self, peer: &PeerId) {
        self.connected_peers.remove(peer);
    }
    fn on_address_update(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        if let Some(record) = self.address_book.get_mut(peer) {
            record.on_address_update(address)
        } else {
            self.address_book
                .insert(*peer, PeerAddressRecord::new(address));
            true
        }
    }
    fn list_connected(&self) -> Box<[&PeerId]> {
        self.connected_peers.iter().collect()
    }
    fn addresses_of_peer(&self, peer: &PeerId) -> Option<Box<[crate::AddressRecord]>> {
        self.address_book.get(peer).map(|record| record.records())
    }
}
