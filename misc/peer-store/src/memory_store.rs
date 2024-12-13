use std::{collections::HashMap, num::NonZeroUsize, time::SystemTime};

use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::FromSwarm;
use lru::LruCache;

use super::{store::Event, Store};

pub(crate) struct PeerAddressRecord {
    addresses: LruCache<Multiaddr, AddressRecord>,
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
        let mut address_book = LruCache::new(NonZeroUsize::new(8).expect("8 is greater than 0"));
        address_book.get_or_insert(address.clone(), AddressRecord::new);
        Self {
            addresses: address_book,
        }
    }
    pub(crate) fn update_address(&mut self, address: &Multiaddr) -> bool {
        if let Some(record) = self.addresses.get_mut(address) {
            record.update_last_seen();
            return false;
        }
        // reduce syscall(new record won't call `SystemTime::now()` twice)
        self.addresses
            .get_or_insert(address.clone(), AddressRecord::new);
        true
    }
    pub(crate) fn remove_address(&mut self, address: &Multiaddr) -> bool {
        self.addresses.pop(address).is_some()
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
    fn update_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        if let Some(record) = self.address_book.get_mut(peer) {
            record.update_address(address)
        } else {
            self.address_book
                .insert(*peer, PeerAddressRecord::new(address));
            true
        }
    }
    fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        if let Some(record) = self.address_book.get_mut(peer) {
            return record.remove_address(address);
        }
        false
    }
    fn on_swarm_event(&mut self, swarm_event: &FromSwarm) -> Option<Event> {
        match swarm_event {
            FromSwarm::NewExternalAddrOfPeer(info) => {
                if self.update_address(&info.peer_id, info.addr) {
                    return Some(Event::RecordUpdated(info.peer_id));
                }
                None
            }
            FromSwarm::ConnectionEstablished(info) => {
                let mut is_record_updated = false;
                for failed_addr in info.failed_addresses {
                    is_record_updated |= self.remove_address(&info.peer_id, failed_addr);
                }
                is_record_updated |=
                    self.update_address(&info.peer_id, info.endpoint.get_remote_address());
                if is_record_updated {
                    return Some(Event::RecordUpdated(info.peer_id));
                }
                None
            }
            _ => None,
        }
    }
    fn addresses_of_peer(
        &self,
        peer: &PeerId,
    ) -> Option<impl Iterator<Item = super::AddressRecord>> {
        self.address_book.get(peer).map(|record| record.records())
    }
}
