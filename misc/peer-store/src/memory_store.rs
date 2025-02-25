//! An in-memory [`Store`] implementation.
//!
//! ## Usage
//! ```
//! use libp2p_peer_store::{memory_store::MemoryStore, Behaviour};
//!
//! let store: MemoryStore<()> = MemoryStore::new(Default::default());
//! let behaviour = Behaviour::new(store);
//! ```
//!
//! ## Persistent storage
//! [`MemoryStore`] keeps all records in memory and will lose all the data
//! once de-allocated. In order to persist the data, enable `serde` feature
//! of `lib2p-peer-store` to read from and write to disk.

use std::{
    collections::{HashMap, VecDeque},
    num::NonZeroUsize,
    task::Waker,
};

#[cfg(feature = "serde")]
use ::serde::{Deserialize, Serialize};
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::FromSwarm;
use lru::LruCache;

use super::Store;

/// Event from the store and emitted to [`Swarm`](libp2p_swarm::Swarm).
#[derive(Debug, Clone)]
pub enum Event {
    /// Custom data of the peer has been updated.
    CustomDataUpdated(PeerId),
}

/// A in-memory store that uses LRU cache for bounded storage of addresses
/// and a frequency-based ordering of addresses.
#[derive(Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(bound(
        serialize = "T: Clone + Serialize ",
        deserialize = "T: Clone + Deserialize<'de>"
    ))
)]
pub struct MemoryStore<T = ()> {
    /// The internal store.
    records: HashMap<PeerId, PeerRecord<T>>,
    /// Events to emit to [`Behaviour`](crate::Behaviour) and [`Swarm`](libp2p_swarm::Swarm)
    #[cfg_attr(feature = "serde", serde(skip))]
    pending_events: VecDeque<crate::store::Event<Event>>,
    /// Config of the store.
    config: Config,
    /// Waker for store events.
    #[cfg_attr(feature = "serde", serde(skip))]
    waker: Option<Waker>,
}

impl<T> MemoryStore<T> {
    /// Create a new [`MemoryStore`] with the given config.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            records: HashMap::new(),
            pending_events: VecDeque::default(),
            waker: None,
        }
    }

    /// Update an address record and notify swarm when the address is new.  
    /// Returns `true` when the address is new.  
    pub fn update_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        let is_updated = self.update_address_silent(peer, address);
        if is_updated {
            self.pending_events
                .push_back(crate::store::Event::RecordUpdated(*peer));
            if let Some(waker) = self.waker.take() {
                waker.wake();
            }
        }
        is_updated
    }

    /// Update an address record without notifying swarm.  
    /// Returns `true` when the address is new.  
    pub fn update_address_silent(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        if let Some(record) = self.records.get_mut(peer) {
            return record.update_address(address);
        }
        let mut new_record = PeerRecord::new(self.config.record_capacity);
        new_record.update_address(address);
        self.records.insert(*peer, new_record);
        true
    }

    /// Remove an address record.
    /// Returns `true` when the address is removed.
    pub fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        self.records
            .get_mut(peer)
            .is_some_and(|r| r.remove_address(address))
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
            .push_back(crate::store::Event::Store(Event::CustomDataUpdated(*peer)));
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
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
    /// Will not wake up the task.
    pub fn record_iter_mut(&mut self) -> impl Iterator<Item = (&PeerId, &mut PeerRecord<T>)> {
        self.records.iter_mut()
    }
}

impl<T> Store for MemoryStore<T> {
    type FromStore = Event;

    fn on_swarm_event(&mut self, swarm_event: &FromSwarm) {
        match swarm_event {
            FromSwarm::NewExternalAddrOfPeer(info) => {
                self.update_address(&info.peer_id, info.addr);
            }
            FromSwarm::ConnectionEstablished(info) => {
                let mut is_record_updated = false;
                for failed_addr in info.failed_addresses {
                    is_record_updated |= self.remove_address(&info.peer_id, failed_addr);
                }
                is_record_updated |=
                    self.update_address_silent(&info.peer_id, info.endpoint.get_remote_address());
                if is_record_updated {
                    self.pending_events
                        .push_back(crate::store::Event::RecordUpdated(info.peer_id));
                    if let Some(waker) = self.waker.take() {
                        waker.wake(); // wake up because of update
                    }
                }
            }
            _ => {}
        }
    }

    fn addresses_of_peer(&self, peer: &PeerId) -> Option<impl Iterator<Item = &Multiaddr>> {
        self.records.get(peer).map(|record| record.addresses())
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Option<crate::store::Event<Self::FromStore>> {
        if self.pending_events.is_empty() {
            self.waker = Some(cx.waker().clone());
        }
        self.pending_events.pop_front()
    }
}

/// Config for [`MemoryStore`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Config {
    /// The capacaity of an address store.  
    /// The least active address will be discarded to make room for new address.
    record_capacity: NonZeroUsize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            record_capacity: NonZeroUsize::try_from(8).expect("8 > 0"),
        }
    }
}

impl Config {
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
}

/// Internal record of [`MemoryStore`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(bound(
        serialize = "T: Clone + Serialize ",
        deserialize = "T: Clone + Deserialize<'de>"
    ))
)]
#[cfg_attr(feature = "serde", serde(from = "serde::PeerRecord<T>"))]
#[cfg_attr(feature = "serde", serde(into = "serde::PeerRecord<T>"))]
pub struct PeerRecord<T> {
    /// A LRU(Least Recently Used) cache for addresses.  
    /// Will delete the least-recently-used record when full.
    addresses: LruCache<Multiaddr, ()>,
    /// Custom data attached to the peer.
    custom_data: Option<T>,
}
impl<T> PeerRecord<T> {
    pub(crate) fn new(cap: NonZeroUsize) -> Self {
        Self {
            addresses: LruCache::new(cap),
            custom_data: None,
        }
    }

    /// Iterate over all addresses. More recently-used address comes first.
    /// Does not change the order.
    pub fn addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.addresses.iter().map(|(addr, _)| addr)
    }

    /// Update the address in the LRU cache, promote it to the front if it exists,
    /// insert it to the front if not.
    /// Returns true when the address is new.
    pub fn update_address(&mut self, address: &Multiaddr) -> bool {
        if self.addresses.get(address).is_some() {
            return false;
        }
        self.addresses.get_or_insert(address.clone(), || ());
        true
    }

    /// Remove the address in the LRU cache regardless of its position.
    /// Returns true when the address is removed, false when not exist.
    pub fn remove_address(&mut self, address: &Multiaddr) -> bool {
        self.addresses.pop(address).is_some()
    }

    pub fn get_custom_data(&self) -> Option<&T> {
        self.custom_data.as_ref()
    }

    pub fn take_custom_data(&mut self) -> Option<T> {
        self.custom_data.take()
    }

    pub fn insert_custom_data(&mut self, custom_data: T) {
        let _ = self.custom_data.insert(custom_data);
    }
}

#[cfg(feature = "serde")]
pub(crate) mod serde {
    use std::num::NonZeroUsize;

    use libp2p_core::Multiaddr;
    use serde::{Deserialize, Serialize};

    impl<T> super::PeerRecord<T> {
        /// Build from an iterator. The order is reversed(FILO-ish).
        pub(crate) fn from_iter(
            cap: NonZeroUsize,
            addr_iter: impl Iterator<Item = Multiaddr>,
            custom_data: Option<T>,
        ) -> Self {
            let mut lru = lru::LruCache::new(cap);
            for addr in addr_iter {
                lru.get_or_insert(addr, || ());
            }
            Self {
                addresses: lru,
                custom_data,
            }
        }

        pub(crate) fn destruct(self) -> (NonZeroUsize, Vec<Multiaddr>, Option<T>) {
            let cap = self.addresses.cap();
            let mut addresses = self
                .addresses
                .into_iter()
                .map(|(addr, _)| addr)
                .collect::<Vec<_>>();
            // This is somewhat unusual: `LruCache::iter()` retains LRU order
            // while `LruCache::into_iter()` reverses the order.
            addresses.reverse();
            (cap, addresses, self.custom_data)
        }
    }

    /// Helper struct for serializing and deserializing [`PeerRecord`](super::PeerRecord)
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PeerRecord<T> {
        pub addresses: Vec<Multiaddr>,
        pub cap: NonZeroUsize,
        pub custom_data: Option<T>,
    }
    impl<T: Clone> From<PeerRecord<T>> for super::PeerRecord<T> {
        fn from(value: PeerRecord<T>) -> Self {
            // Need to reverse the iterator because `LruCache` is FILO.
            super::PeerRecord::from_iter(
                value.cap,
                value.addresses.into_iter().rev(),
                value.custom_data,
            )
        }
    }
    impl<T: Clone> From<super::PeerRecord<T>> for PeerRecord<T> {
        fn from(value: super::PeerRecord<T>) -> Self {
            let (cap, addresses, custom_data) = value.destruct();
            PeerRecord {
                addresses,
                cap,
                custom_data,
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{num::NonZero, str::FromStr};

    use libp2p::Swarm;
    use libp2p_core::{Multiaddr, PeerId};
    use libp2p_swarm::{NetworkBehaviour, SwarmEvent};
    use libp2p_swarm_test::SwarmExt;

    use super::MemoryStore;
    use crate::Store;

    #[test]
    fn recent_use_bubble_up() {
        let mut store: MemoryStore = MemoryStore::new(Default::default());
        let peer = PeerId::random();
        let addr1 = Multiaddr::from_str("/ip4/127.0.0.1").expect("parsing to succeed");
        let addr2 = Multiaddr::from_str("/ip4/127.0.0.2").expect("parsing to succeed");
        let addr3 = Multiaddr::from_str("/ip4/127.0.0.3").expect("parsing to succeed");
        store.update_address(&peer, &addr1);
        store.update_address(&peer, &addr2);
        store.update_address(&peer, &addr3);
        assert!(
            store
                .records
                .get(&peer)
                .expect("peer to be in the store")
                .addresses()
                .collect::<Vec<_>>()
                == vec![&addr3, &addr2, &addr1]
        );
        store.update_address(&peer, &addr1);
        assert!(
            store
                .records
                .get(&peer)
                .expect("peer to be in the store")
                .addresses()
                .collect::<Vec<_>>()
                == vec![&addr1, &addr3, &addr2]
        );
        store.update_address(&peer, &addr3);
        assert!(
            store
                .records
                .get(&peer)
                .expect("peer to be in the store")
                .addresses()
                .collect::<Vec<_>>()
                == vec![&addr3, &addr1, &addr2]
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
            );
        }
        let first_record = Multiaddr::from_str("/ip4/127.0.0.1").expect("parsing to succeed");
        assert!(!store
            .addresses_of_peer(&peer)
            .expect("peer to be in the store")
            .any(|addr| *addr == first_record));
        let second_record = Multiaddr::from_str("/ip4/127.0.0.2").expect("parsing to succeed");
        assert!(
            *store
                .addresses_of_peer(&peer)
                .expect("peer to be in the store")
                .last()
                .expect("addr to exist")
                == second_record
        );
    }

    #[test]
    fn serde_roundtrip() {
        let store_json = r#"
    {
        "records": {
            "1Aea5mXJrZNUwKxNU2y9xFE2qTFMjvFYSBf4T8cWEEP5Zd": 
                {
                    "addresses": [ "/ip4/127.0.0.2", "/ip4/127.0.0.3", "/ip4/127.0.0.4",
                                   "/ip4/127.0.0.5", "/ip4/127.0.0.6", "/ip4/127.0.0.7",
                                   "/ip4/127.0.0.8", "/ip4/127.0.0.9" ],
                    "cap": 8,
                    "custom_data": 7
                }
        },
        "config": {
            "record_capacity": 8
        }
    }
        "#;
        let store: MemoryStore<u32> = serde_json::from_str(store_json).unwrap();
        let peer = PeerId::from_str("1Aea5mXJrZNUwKxNU2y9xFE2qTFMjvFYSBf4T8cWEEP5Zd")
            .expect("Parsing to succeed.");
        let mut addresses = Vec::new();
        for i in 2..10 {
            let addr_string = format!("/ip4/127.0.0.{}", i);
            addresses.push(Multiaddr::from_str(&addr_string).expect("parsing to succeed"));
        }
        // should retain order when deserializing from bytes.
        assert_eq!(
            store
                .addresses_of_peer(&peer)
                .expect("Peer to exist")
                .cloned()
                .collect::<Vec<_>>(),
            addresses
        );
        assert_eq!(*store.get_custom_data(&peer).expect("Peer to exist"), 7);
        let ser = serde_json::to_string(&store).expect("Serialize to succeed.");
        let store_de: MemoryStore<u32> =
            serde_json::from_str(&ser).expect("Deserialize to succeed");
        // should retain order when serializing
        assert_eq!(
            store_de
                .addresses_of_peer(&peer)
                .expect("Peer to exist")
                .cloned()
                .collect::<Vec<_>>(),
            addresses
        );
        assert_eq!(*store_de.get_custom_data(&peer).expect("Peer to exist"), 7);
    }

    #[test]
    fn update_address_on_connect() {
        async fn expect_record_update(
            swarm: &mut Swarm<crate::Behaviour<MemoryStore>>,
            expected_peer: PeerId,
        ) {
            swarm
                .wait(|ev| match ev {
                    SwarmEvent::Behaviour(crate::Event::RecordUpdated { peer }) => {
                        (peer == expected_peer).then_some(())
                    }
                    _ => None,
                })
                .await
        }

        let store1: MemoryStore<()> = MemoryStore::new(
            crate::memory_store::Config::default()
                .set_record_capacity(NonZero::new(2).expect("2 > 0")),
        );
        let mut swarm1 = Swarm::new_ephemeral_tokio(|_| crate::Behaviour::new(store1));
        let store2: MemoryStore<()> = MemoryStore::new(
            crate::memory_store::Config::default()
                .set_record_capacity(NonZero::new(2).expect("2 > 0")),
        );
        let mut swarm2 = Swarm::new_ephemeral_tokio(|_| crate::Behaviour::new(store2));

        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let (listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            let swarm1_peer_id = *swarm1.local_peer_id();
            swarm2.dial(listen_addr.clone()).expect("dial to succeed");
            let handle = spawn_wait_conn_established(swarm1);
            swarm2
                .wait(|ev| match ev {
                    SwarmEvent::ConnectionEstablished { .. } => Some(()),
                    _ => None,
                })
                .await;
            let mut swarm1 = handle.await.expect("future to complete");
            assert!(swarm2
                .behaviour()
                .address_of_peer(&swarm1_peer_id)
                .expect("swarm should be connected and record about it should be created")
                .any(|addr| *addr == listen_addr));
            expect_record_update(&mut swarm1, *swarm2.local_peer_id()).await;
            let (new_listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            let handle = spawn_wait_conn_established(swarm1);
            swarm2
                .dial(
                    libp2p_swarm::dial_opts::DialOpts::peer_id(swarm1_peer_id)
                        .condition(libp2p_swarm::dial_opts::PeerCondition::Always)
                        .addresses(vec![new_listen_addr.clone()])
                        .build(),
                )
                .expect("dial to succeed");
            swarm2
                .wait(|ev| match ev {
                    SwarmEvent::ConnectionEstablished { .. } => Some(()),
                    _ => None,
                })
                .await;
            handle.await.expect("future to complete");
            expect_record_update(&mut swarm2, swarm1_peer_id).await;
            // The address in store will contain peer ID.
            let new_listen_addr = new_listen_addr
                .with_p2p(swarm1_peer_id)
                .expect("extend to succeed");
            assert!(
                swarm2
                    .behaviour()
                    .address_of_peer(&swarm1_peer_id)
                    .expect("peer to exist")
                    .collect::<Vec<_>>()
                    == vec![&new_listen_addr, &listen_addr]
            );
        })
    }

    #[test]
    fn identify_external_addr_report() {
        #[derive(NetworkBehaviour)]
        struct Behaviour {
            peer_store: crate::Behaviour<MemoryStore>,
            identify: libp2p::identify::Behaviour,
        }
        async fn expect_record_update(swarm: &mut Swarm<Behaviour>, expected_peer: PeerId) {
            swarm
                .wait(|ev| match ev {
                    SwarmEvent::Behaviour(BehaviourEvent::PeerStore(
                        crate::Event::RecordUpdated { peer },
                    )) => (peer == expected_peer).then_some(()),
                    _ => None,
                })
                .await
        }
        fn build_swarm() -> Swarm<Behaviour> {
            Swarm::new_ephemeral_tokio(|kp| Behaviour {
                peer_store: crate::Behaviour::new(MemoryStore::new(
                    crate::memory_store::Config::default()
                        .set_record_capacity(NonZero::new(4).expect("4 > 0")),
                )),
                identify: libp2p::identify::Behaviour::new(libp2p::identify::Config::new(
                    "/TODO/0.0.1".to_string(),
                    kp.public(),
                )),
            })
        }
        let mut swarm1 = build_swarm();
        let mut swarm2 = build_swarm();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let (listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            let swarm1_peer_id = *swarm1.local_peer_id();
            let swarm2_peer_id = *swarm2.local_peer_id();
            swarm2.dial(listen_addr.clone()).expect("dial to succeed");
            let handle = spawn_wait_conn_established(swarm1);
            let mut swarm2 = spawn_wait_conn_established(swarm2)
                .await
                .expect("future to complete");
            let mut swarm1 = handle.await.expect("future to complete");
            // expexting update from direct connection.
            expect_record_update(&mut swarm2, swarm1_peer_id).await;
            assert!(swarm2
                .behaviour()
                .peer_store
                .address_of_peer(&swarm1_peer_id)
                .expect("swarm should be connected and record about it should be created")
                .any(|addr| *addr == listen_addr));
            expect_record_update(&mut swarm1, *swarm2.local_peer_id()).await;
            swarm1.next_swarm_event().await; // skip `identify::Event::Sent`
            swarm1.next_swarm_event().await; // skip `identify::Event::Received`
            let (new_listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            swarm1.behaviour_mut().identify.push([swarm2_peer_id]);
            tokio::spawn(swarm1.loop_on_next());
            // Expecting 3 updates from Identify:
            // 2 pair of mem and tcp address for two calls to `<Swarm as SwarmExt>::listen()`
            // with one address already present through direct connection.
            // FLAKY: tcp addresses are not explicitly marked as external addresses.
            expect_record_update(&mut swarm2, swarm1_peer_id).await;
            expect_record_update(&mut swarm2, swarm1_peer_id).await;
            expect_record_update(&mut swarm2, swarm1_peer_id).await;
            // The address in store won't contain peer ID because it is from Identify.
            assert!(swarm2
                .behaviour()
                .peer_store
                .address_of_peer(&swarm1_peer_id)
                .expect("peer to exist")
                .any(|addr| *addr == new_listen_addr));
        })
    }

    fn spawn_wait_conn_established<T>(mut swarm: Swarm<T>) -> tokio::task::JoinHandle<Swarm<T>>
    where
        T: NetworkBehaviour + Send + Sync,
        Swarm<T>: SwarmExt,
    {
        tokio::spawn(async move {
            swarm
                .wait(|ev| match ev {
                    SwarmEvent::ConnectionEstablished { .. } => Some(()),
                    _ => None,
                })
                .await;
            swarm
        })
    }
}
