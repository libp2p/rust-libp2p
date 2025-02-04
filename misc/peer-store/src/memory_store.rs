use std::{
    collections::{HashMap, VecDeque},
    num::NonZeroUsize,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures_timer::Delay;
use futures_util::FutureExt;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::FromSwarm;
use record::PeerRecord;

use super::Store;
use crate::{store::AddressSource, Behaviour};

#[derive(Debug, Clone)]
pub enum Event {
    CustomDataUpdated(PeerId),
}

/// A in-memory store.
#[derive(Default)]
pub struct MemoryStore<T = ()> {
    /// The internal store.
    records: HashMap<PeerId, record::PeerRecord<T>>,
    pending_events: VecDeque<super::store::Event<Event>>,
    record_ttl_timer: Option<Delay>,
    config: Config,
}

impl<T> MemoryStore<T> {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            records: HashMap::new(),
            record_ttl_timer: None,
            pending_events: VecDeque::default(),
        }
    }

    pub fn get_custom_data(&self, peer: &PeerId) -> Option<&T> {
        self.records.get(peer).and_then(|r| r.get_custom_data())
    }

    /// Take ownership of the internal data, leaving `None` in its place.
    pub fn take_custom_data(&mut self, peer: &PeerId) -> Option<T> {
        self.records
            .get_mut(peer)
            .and_then(|r| r.take_custom_data())
    }

    /// Insert the data and notify the swarm about the update, dropping the old data if it exists.
    pub fn insert_custom_data(&mut self, peer: &PeerId, custom_data: T) {
        self.insert_custom_data_silent(peer, custom_data);
        self.pending_events
            .push_back(super::store::Event::Store(Event::CustomDataUpdated(*peer)));
    }

    /// Insert the data without notifying the swarm. Old data will be dropped if it exists.
    pub fn insert_custom_data_silent(&mut self, peer: &PeerId, custom_data: T) {
        if let Some(r) = self.records.get_mut(peer) {
            return r.insert_custom_data(custom_data);
        }
        let mut new_record = PeerRecord::new(self.config.record_capacity);
        new_record.insert_custom_data(custom_data);
        self.records.insert(*peer, new_record);
    }

    /// Iterate over all internal records.
    pub fn record_iter(&self) -> impl Iterator<Item = (&PeerId, &PeerRecord<T>)> {
        self.records.iter()
    }

    /// Iterate over all internal records mutably.
    pub fn record_iter_mut(&mut self) -> impl Iterator<Item = (&PeerId, &mut PeerRecord<T>)> {
        self.records.iter_mut()
    }

    fn check_record_ttl(&mut self) {
        let now = Instant::now();
        for r in &mut self.records.values_mut() {
            r.check_addresses_ttl(now, self.config.record_ttl);
        }
    }
}

impl<T> Store for MemoryStore<T> {
    type FromStore = Event;

    fn update_address(
        &mut self,
        peer: &PeerId,
        address: &Multiaddr,
        source: AddressSource,
        should_expire: bool,
    ) -> bool {
        if let Some(record) = self.records.get_mut(peer) {
            return record.update_address(address, source, should_expire);
        }
        let mut new_record = record::PeerRecord::new(self.config.record_capacity);
        new_record.update_address(address, source, should_expire);
        self.records.insert(*peer, new_record);
        true
    }

    fn update_certified_address(
        &mut self,
        signed_record: &libp2p_core::PeerRecord,
        source: AddressSource,
        should_expire: bool,
    ) -> bool {
        let peer = signed_record.peer_id();
        if let Some(record) = self.records.get_mut(&peer) {
            return record.update_certified_address(signed_record, source, should_expire);
        }
        let mut new_record = record::PeerRecord::new(self.config.record_capacity);
        new_record.update_certified_address(signed_record, source, should_expire);
        self.records.insert(peer, new_record);
        true
    }

    fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        if let Some(record) = self.records.get_mut(peer) {
            return record.remove_address(address);
        }
        false
    }

    fn on_swarm_event(&mut self, swarm_event: &FromSwarm) {
        match swarm_event {
            FromSwarm::NewExternalAddrOfPeer(info) => {
                if self.update_address(&info.peer_id, info.addr, AddressSource::Behaviour, true) {
                    self
                        .pending_events
                        .push_back(super::store::Event::RecordUpdated(info.peer_id));
                }
            }
            FromSwarm::ConnectionEstablished(info) => {
                let mut is_record_updated = false;
                for failed_addr in info.failed_addresses {
                    is_record_updated |= self.remove_address(&info.peer_id, failed_addr);
                }
                is_record_updated |= self.update_address(
                    &info.peer_id,
                    info.endpoint.get_remote_address(),
                    AddressSource::DirectConnection,
                    false,
                );
                if is_record_updated {
                    self.pending_events
                        .push_back(super::store::Event::RecordUpdated(info.peer_id));
                }
            }
            _ => {},
        }
    }

    fn addresses_of_peer(&self, peer: &PeerId) -> Option<impl Iterator<Item = &Multiaddr>> {
        self.records.get(peer).map(|record| {
            record
                .addresses()
                .filter(|(_, r)| !self.config.strict_mode || r.signature.is_some())
                .map(|(addr, _)| addr)
        })
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Option<super::store::Event<Self::FromStore>> {
        if let Some(mut timer) = self.record_ttl_timer.take() {
            if let Poll::Ready(()) = timer.poll_unpin(cx) {
                self.check_record_ttl();
                self.record_ttl_timer = Some(Delay::new(self.config.check_record_ttl_interval));
            }
            self.record_ttl_timer = Some(timer)
        }
        if let Some(ev) = self.pending_events.pop_front() {
            return Some(ev);
        }
        None
    }
}

impl<T> Behaviour<MemoryStore<T>>
where
    T: 'static,
{
    /// Get all stored address records of the peer, not affected by `strict_mode`.
    pub fn address_record_of_peer(
        &self,
        peer: &PeerId,
    ) -> Option<impl Iterator<Item = (&Multiaddr, &record::AddressRecord)>> {
        self.store().records.get(peer).map(|r| r.addresses())
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// TTL of an address record.
    record_ttl: Duration,
    /// The capacaity of an address store.  
    /// The least active address will be discarded to make room for new address.
    record_capacity: NonZeroUsize,
    /// The interval for garbage collecting records.
    check_record_ttl_interval: Duration,
    /// Only provide signed addresses to the behaviour when set to true.
    strict_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            record_ttl: Duration::from_secs(600),
            record_capacity: NonZeroUsize::try_from(8).expect("8 > 0"),
            check_record_ttl_interval: Duration::from_secs(5),
            strict_mode: false,
        }
    }
}

impl Config {
    /// TTL of all address records.
    pub fn record_ttl(&self) -> &Duration {
        &self.record_ttl
    }
    /// Set TTL for all address records.
    pub fn set_record_ttl(mut self, ttl: Duration) -> Self {
        self.record_ttl = ttl;
        self
    }
    /// Capacity for address records.
    /// The least active address will be dropped to make room for new address.
    pub fn record_capacity(&self) -> &NonZeroUsize {
        &self.record_capacity
    }
    /// Set the capacity for address records.
    pub fn set_record_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.record_capacity = capacity;
        self
    }
    /// The interval for garbage collecting records.
    pub fn check_record_ttl_interval(&self) -> &Duration {
        &self.check_record_ttl_interval
    }
    /// Set the interval for garbage collecting records.
    pub fn set_check_record_ttl_interval(mut self, interval: Duration) -> Self {
        self.check_record_ttl_interval = interval;
        self
    }
    /// Only provide signed addresses to the behaviour when true.
    pub fn is_strict_mode(&self) -> bool {
        self.strict_mode
    }
    /// Set `strict_mode`.
    pub fn set_strict_mode(mut self, is_strict: bool) -> Self {
        self.strict_mode = is_strict;
        self
    }
}
mod record {
    use std::sync::Arc;

    use lru::LruCache;

    use super::*;

    pub struct PeerRecord<T> {
        /// A LRU(Least Recently Used) cache for addresses.  
        /// Will delete the least-recently-used record when full.
        addresses: LruCache<Multiaddr, AddressRecord>,
        custom: Option<T>,
    }
    impl<T> PeerRecord<T> {
        pub(crate) fn new(capacity: NonZeroUsize) -> Self {
            Self {
                addresses: LruCache::new(capacity),
                custom: None,
            }
        }

        pub(crate) fn addresses(&self) -> impl Iterator<Item = (&Multiaddr, &AddressRecord)> {
            self.addresses.iter()
        }

        pub(crate) fn update_address(
            &mut self,
            address: &Multiaddr,
            source: AddressSource,
            should_expire: bool,
        ) -> bool {
            if let Some(record) = self.addresses.get_mut(address) {
                record.update_last_seen();
                return false;
            }
            // new record won't call `Instant::now()` twice
            self.addresses.get_or_insert(address.clone(), || {
                AddressRecord::new(source, should_expire, None)
            });
            true
        }

        pub(crate) fn update_certified_address(
            &mut self,
            signed_record: &libp2p_core::PeerRecord,
            source: AddressSource,
            should_expire: bool,
        ) -> bool {
            let mut is_updated = false;
            let signed_record = Arc::new(signed_record.clone());
            for address in signed_record.addresses() {
                // promote the address or update with the latest signature.
                if let Some(r) = self.addresses.get_mut(address) {
                    r.signature = Some(signed_record.clone());
                    continue;
                }
                // the address is not present. this defers cloning.
                self.addresses.get_or_insert(address.clone(), || {
                    AddressRecord::new(source, should_expire, Some(signed_record.clone()))
                });
                is_updated = true;
            }
            is_updated
        }

        pub(crate) fn remove_address(&mut self, address: &Multiaddr) -> bool {
            self.addresses.pop(address).is_some()
        }

        pub(crate) fn check_addresses_ttl(&mut self, now: Instant, ttl: Duration) {
            let mut records_to_be_deleted = Vec::new();
            for (k, record) in self.addresses.iter() {
                if record.is_expired(now, ttl) {
                    records_to_be_deleted.push(k.clone());
                }
            }
            for k in records_to_be_deleted {
                self.addresses.pop(&k);
            }
        }

        pub(crate) fn get_custom_data(&self) -> Option<&T> {
            self.custom.as_ref()
        }

        pub(crate) fn take_custom_data(&mut self) -> Option<T> {
            self.custom.take()
        }

        pub(crate) fn insert_custom_data(&mut self, custom_data: T) {
            let _ = self.custom.insert(custom_data);
        }
    }

    pub struct AddressRecord {
        /// The time when the address is last seen.
        pub last_seen: Instant,
        /// How the address is discovered.
        pub source: AddressSource,
        /// Whether the address will expire.
        pub should_expire: bool,
        /// Reference to the `PeerRecord` that contains this address.  
        /// The inner `PeerRecord` will be dropped automatically
        /// when there is no living reference to it.
        pub signature: Option<Arc<libp2p_core::PeerRecord>>,
    }
    impl AddressRecord {
        pub(crate) fn new(
            source: AddressSource,
            should_expire: bool,
            signed: Option<Arc<libp2p_core::PeerRecord>>,
        ) -> Self {
            Self {
                last_seen: Instant::now(),
                source,
                should_expire,
                signature: signed,
            }
        }
        pub(crate) fn update_last_seen(&mut self) {
            self.last_seen = Instant::now();
        }
        pub(crate) fn is_expired(&self, now: Instant, ttl: Duration) -> bool {
            self.should_expire && now.duration_since(self.last_seen) > ttl
        }
    }
}

#[cfg(test)]
mod test {
    use std::{num::NonZeroUsize, str::FromStr, thread, time::Duration};

    use libp2p_core::{Multiaddr, PeerId};

    use super::{Config, MemoryStore};
    use crate::Store;

    #[test]
    fn record_expire() {
        let config = Config {
            record_capacity: NonZeroUsize::try_from(4).expect("4 > 0"),
            record_ttl: Duration::from_millis(1),
            ..Default::default()
        };
        let mut store: MemoryStore = MemoryStore::new(config);
        let peer = PeerId::random();
        let addr_no_expire = Multiaddr::from_str("/ip4/127.0.0.1").expect("parsing to succeed");
        let addr_should_expire = Multiaddr::from_str("/ip4/127.0.0.2").expect("parsing to succeed");
        store.update_address(
            &peer,
            &addr_no_expire,
            crate::store::AddressSource::Manual,
            false,
        );
        store.update_address(
            &peer,
            &addr_should_expire,
            crate::store::AddressSource::Manual,
            true,
        );
        thread::sleep(Duration::from_millis(2));
        store.check_record_ttl();
        assert!(!store
            .addresses_of_peer(&peer)
            .expect("peer to be in the store")
            .any(|r| *r == addr_should_expire));
        assert!(store
            .addresses_of_peer(&peer)
            .expect("peer to be in the store")
            .any(|r| *r == addr_no_expire));
    }

    #[test]
    fn recent_use_bubble_up() {
        let mut store: MemoryStore = MemoryStore::new(Default::default());
        let peer = PeerId::random();
        let addr1 = Multiaddr::from_str("/ip4/127.0.0.1").expect("parsing to succeed");
        let addr2 = Multiaddr::from_str("/ip4/127.0.0.2").expect("parsing to succeed");
        store.update_address(&peer, &addr1, crate::store::AddressSource::Manual, false);
        store.update_address(&peer, &addr2, crate::store::AddressSource::Manual, false);
        assert!(
            *store
                .records
                .get(&peer)
                .expect("peer to be in the store")
                .addresses()
                .last()
                .expect("addr in the record")
                .0
                == addr1
        );
        store.update_address(&peer, &addr1, crate::store::AddressSource::Manual, false);
        assert!(
            *store
                .records
                .get(&peer)
                .expect("peer to be in the store")
                .addresses()
                .last()
                .expect("addr in the record")
                .0
                == addr2
        );
    }

    #[test]
    fn bounded_store() {
        let mut store: MemoryStore = MemoryStore::new(Default::default());
        let peer = PeerId::random();
        for i in 1..10 {
            let addr_string = format!("/ip4/127.0.0.{}", i);
            store.update_address(
                &peer,
                &Multiaddr::from_str(&addr_string).expect("parsing to succeed"),
                crate::store::AddressSource::Manual,
                false,
            );
        }
        let first_record = Multiaddr::from_str("/ip4/127.0.0.1").expect("parsing to succeed");
        assert!(!store
            .addresses_of_peer(&peer)
            .expect("peer to be in the store")
            .any(|addr| *addr == first_record));
        let second_record = Multiaddr::from_str("/ip4/127.0.0.2").expect("parsing to succeed");
        assert!(store
            .addresses_of_peer(&peer)
            .expect("peer to be in the store")
            .any(|addr| *addr == second_record));
    }
}
