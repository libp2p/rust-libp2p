//! An in-memory [`Store`] implementation.
//!
//! ## Usage
//! ```
//! use libp2p_peer_store::{memory_store::MemoryStore, Behaviour};
//!
//! let store: MemoryStore<()> = MemoryStore::new(Default::default());
//! let behaviour = Behaviour::new(store);
//! ```

use std::{
    collections::VecDeque,
    num::NonZeroUsize,
    task::{Poll, Waker},
};

use hashlink::LruCache;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{behaviour::ConnectionEstablished, DialError, FromSwarm};

use super::Store;

/// Event emitted from the [`MemoryStore`] to the [`Swarm`](libp2p_swarm::Swarm).
#[derive(Debug, Clone)]
pub enum Event {
    /// A new peer address has been added to the store.
    PeerAddressAdded {
        /// ID of the peer for which the address was added.
        peer_id: PeerId,
        /// The added address.
        address: Multiaddr,
        /// Whether the address will be kept in the store after a dial-failure.
        ///
        /// Set to `true` when an address was added explicitly through
        /// [`MemoryStore::add_address`], `false` if the address was discovered through the
        /// swarm or other behaviors.
        ///
        /// Only relevant when [`Config::is_remove_addr_on_dial_error`] is `true`.
        is_permanent: bool,
    },
    /// A peer address has been removed from the store.
    PeerAddressRemoved {
        /// ID of the peer for which the address was removed.
        peer_id: PeerId,
        /// The removed address.
        address: Multiaddr,
    },
}

/// A in-memory store that uses LRU cache for bounded storage of addresses
/// and a frequency-based ordering of addresses.
pub struct MemoryStore<T = ()> {
    /// The internal store.
    records: LruCache<PeerId, PeerRecord<T>>,
    /// Events to emit to [`Behaviour`](crate::Behaviour) and [`Swarm`](libp2p_swarm::Swarm).
    pending_events: VecDeque<Event>,
    /// Config of the store.
    config: Config,
    /// Waker for store events.
    waker: Option<Waker>,
}

impl<T> MemoryStore<T> {
    /// Create a new [`MemoryStore`] with the given config.
    pub fn new(config: Config) -> Self {
        Self {
            records: LruCache::new(config.peer_capacity().get()),
            config,
            pending_events: VecDeque::default(),
            waker: None,
        }
    }

    /// Add an address for a peer.
    ///
    /// The added address will NOT be removed from the store on dial failure. If the added address
    /// is supposed to be cleared from the store on dial failure, add it by emitting
    /// [`FromSwarm::NewExternalAddrOfPeer`] to the swarm, e.g. via
    /// [`Swarm::add_peer_address`](libp2p_swarm::Swarm::add_peer_address).
    ///
    /// Returns `true` if the address is new.
    pub fn add_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        self.add_address_inner(peer, address, true)
    }

    /// Update an address record and notify the swarm.
    ///
    /// Returns `true` if the address is new.
    fn add_address_inner(
        &mut self,
        peer: &PeerId,
        address: &Multiaddr,
        is_permanent: bool,
    ) -> bool {
        let record = self
            .records
            .entry(*peer)
            .or_insert_with(|| PeerRecord::new(self.config.record_capacity));
        let is_new = record.add_address(address, is_permanent);
        if is_new {
            self.push_event_and_wake(Event::PeerAddressAdded {
                peer_id: *peer,
                address: address.clone(),
                is_permanent,
            });
        }
        is_new
    }

    /// Remove an address record.
    ///
    /// Returns `true` when the address existed.
    pub fn remove_address(&mut self, peer: &PeerId, address: &Multiaddr) -> bool {
        self.remove_address_inner(peer, address, true)
    }

    /// Remove an address record and notify the swarm.
    ///
    /// Returns `true` when the address is removed, `false` if the address didn't exist
    /// or the address is permanent and `force` false.
    fn remove_address_inner(&mut self, peer: &PeerId, address: &Multiaddr, force: bool) -> bool {
        if let Some(record) = self.records.get_mut(peer) {
            if record.remove_address(address, force) {
                if record.addresses.is_empty() && record.custom_data.is_none() {
                    self.records.remove(peer);
                }
                self.push_event_and_wake(Event::PeerAddressRemoved {
                    peer_id: *peer,
                    address: address.clone(),
                });
                return true;
            }
        }
        false
    }

    /// Get a reference to a peer's custom data.
    pub fn get_custom_data(&self, peer: &PeerId) -> Option<&T> {
        self.records.peek(peer).and_then(|r| r.get_custom_data())
    }

    /// Take ownership of the internal data, leaving `None` in its place.
    pub fn take_custom_data(&mut self, peer: &PeerId) -> Option<T> {
        if let Some(record) = self.records.get_mut(peer) {
            let data = record.take_custom_data();
            if record.addresses.is_empty() {
                self.records.remove(peer);
            }
            return data;
        }
        None
    }

    /// Insert the data, dropping the old data if it exists.
    pub fn insert_custom_data(&mut self, peer: &PeerId, custom_data: T) {
        if let Some(r) = self.records.get_mut(peer) {
            return r.insert_custom_data(custom_data);
        }
        let mut new_record = PeerRecord::new(self.config.record_capacity);
        new_record.insert_custom_data(custom_data);
        self.records.insert(*peer, new_record);
    }

    /// Get a mutable reference to a peer's custom data.
    pub fn get_custom_data_mut(&mut self, peer: &PeerId) -> Option<&mut T> {
        self.records
            .get_mut(peer)
            .and_then(|r| r.get_custom_data_mut())
    }

    /// Iterate over all internal records.
    pub fn record_iter(&self) -> impl Iterator<Item = (&PeerId, &PeerRecord<T>)> {
        self.records.iter()
    }

    /// Iterate over all internal records mutably.
    ///
    /// Changes to the records will not generate an event.
    pub fn record_iter_mut(&mut self) -> impl Iterator<Item = (&PeerId, &mut PeerRecord<T>)> {
        self.records.iter_mut()
    }

    fn push_event_and_wake(&mut self, event: Event) {
        self.pending_events.push_back(event);
        if let Some(waker) = self.waker.take() {
            waker.wake(); // wake up because of update
        }
    }
}

impl<T> Store for MemoryStore<T> {
    type Event = Event;

    fn on_swarm_event(&mut self, swarm_event: &FromSwarm) {
        match swarm_event {
            FromSwarm::NewExternalAddrOfPeer(info) => {
                self.add_address_inner(&info.peer_id, info.addr, false);
            }
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                failed_addresses,
                endpoint,
                ..
            }) if endpoint.is_dialer() => {
                if self.config.remove_addr_on_dial_error {
                    for failed_addr in *failed_addresses {
                        self.remove_address_inner(peer_id, failed_addr, false);
                    }
                }
                self.add_address_inner(peer_id, endpoint.get_remote_address(), false);
            }
            FromSwarm::DialFailure(info) => {
                if !self.config.remove_addr_on_dial_error {
                    return;
                }

                let Some(peer) = info.peer_id else {
                    // We don't know which peer we are talking about here.
                    return;
                };

                match info.error {
                    DialError::WrongPeerId { obtained, address } => {
                        // The stored peer id is incorrect, remove incorrect and add correct one.
                        if self.remove_address_inner(&peer, address, false) {
                            self.add_address_inner(obtained, address, false);
                        }
                    }
                    DialError::Transport(errors) => {
                        for (addr, _) in errors {
                            self.remove_address_inner(&peer, addr, false);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn addresses_of_peer(&self, peer: &PeerId) -> Option<impl Iterator<Item = &Multiaddr>> {
        self.records.peek(peer).map(|record| record.addresses())
    }

    fn poll(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Self::Event> {
        match self.pending_events.pop_front() {
            Some(ev) => Poll::Ready(ev),
            None => {
                self.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Config for [`MemoryStore`]. The available options are documented via their setters.
#[derive(Debug, Clone)]
pub struct Config {
    peer_capacity: NonZeroUsize,
    record_capacity: NonZeroUsize,
    remove_addr_on_dial_error: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            peer_capacity: NonZeroUsize::try_from(1000).expect("1000 > 0"),
            record_capacity: NonZeroUsize::try_from(8).expect("8 > 0"),
            remove_addr_on_dial_error: true,
        }
    }
}

impl Config {
    pub fn peer_capacity(&self) -> &NonZeroUsize {
        &self.peer_capacity
    }
    /// The capacity of the address store per peer.
    ///
    /// The least recently updated peer will be discarded to make room for a new peer.
    ///
    /// `1000` by default.
    pub fn set_peer_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.peer_capacity = capacity;
        self
    }
    pub fn record_capacity(&self) -> &NonZeroUsize {
        &self.record_capacity
    }
    /// The capacity of the address store per peer.
    ///
    /// The least active address will be discarded to make room for a new address.
    ///
    /// `8` by default.
    pub fn set_record_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.record_capacity = capacity;
        self
    }
    pub fn is_remove_addr_on_dial_error(&self) -> bool {
        self.remove_addr_on_dial_error
    }
    /// If set to `true`, the store will remove addresses if the swarm indicates a dial failure.
    /// More specifically:
    /// - Failed dials indicated in [`ConnectionEstablished`]'s `failed_addresses` will be removed.
    /// - [`DialError::LocalPeerId`] causes the full peer entry to be removed.
    /// - On [`DialError::WrongPeerId`], the address will be removed from the incorrect peer's
    ///   record and re-added to the correct peer's record.
    /// - On [`DialError::Transport`], all failed addresses will be removed.
    ///
    /// If set to `false`, the logic above is not applied and the store only removes addresses
    /// through calls to [`MemoryStore::remove_address`].
    ///
    /// `true` by default.
    pub fn set_remove_addr_on_dial_error(mut self, value: bool) -> Self {
        self.remove_addr_on_dial_error = value;
        self
    }
}

/// Internal record of [`MemoryStore`].
#[derive(Debug, Clone)]
pub struct PeerRecord<T> {
    /// A LRU(Least Recently Used) cache for addresses.
    /// Will delete the least-recently-used record when full.
    /// If the associated `bool` is true, the address can only be force-removed.
    addresses: LruCache<Multiaddr, bool>,
    /// Custom data attached to the peer.
    custom_data: Option<T>,
}
impl<T> PeerRecord<T> {
    pub(crate) fn new(cap: NonZeroUsize) -> Self {
        Self {
            addresses: LruCache::new(cap.get()),
            custom_data: None,
        }
    }

    /// Iterate over all addresses. More recently-used address comes first.
    /// Does not change the order.
    pub fn addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.addresses.iter().rev().map(|(addr, _)| addr)
    }

    /// Update the address in the LRU cache, promote it to the front if it exists,
    /// insert it to the front if not.
    ///
    /// Returns true when the address is new.
    pub fn add_address(&mut self, address: &Multiaddr, is_permanent: bool) -> bool {
        if let Some(was_permanent) = self.addresses.get(address) {
            if !*was_permanent && is_permanent {
                self.addresses.insert(address.clone(), is_permanent);
            }
            false
        } else {
            self.addresses.insert(address.clone(), is_permanent);
            true
        }
    }

    /// Remove the address in the LRU cache regardless of its position.
    ///
    /// Returns true when the address is removed, false when it didn't exist
    /// or it is permanent and `force` is false.
    pub fn remove_address(&mut self, address: &Multiaddr, force: bool) -> bool {
        if !force && self.addresses.peek(address) == Some(&true) {
            return false;
        }
        self.addresses.remove(address).is_some()
    }

    pub fn get_custom_data(&self) -> Option<&T> {
        self.custom_data.as_ref()
    }

    pub fn get_custom_data_mut(&mut self) -> Option<&mut T> {
        self.custom_data.as_mut()
    }

    pub fn take_custom_data(&mut self) -> Option<T> {
        self.custom_data.take()
    }

    pub fn insert_custom_data(&mut self, custom_data: T) {
        let _ = self.custom_data.insert(custom_data);
    }

    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty() && self.custom_data.is_none()
    }
}

#[cfg(test)]
mod test {
    use std::{num::NonZero, str::FromStr};

    use libp2p::identify;
    use libp2p_core::{multiaddr::Protocol, Multiaddr, PeerId};
    use libp2p_swarm::{NetworkBehaviour, Swarm, SwarmEvent};
    use libp2p_swarm_test::SwarmExt;

    use super::{Event, MemoryStore};
    use crate::Store;

    #[test]
    fn recent_use_bubble_up() {
        let mut store: MemoryStore = MemoryStore::new(Default::default());
        let peer = PeerId::random();
        let addr1 = Multiaddr::from_str("/ip4/127.0.0.1").expect("parsing to succeed");
        let addr2 = Multiaddr::from_str("/ip4/127.0.0.2").expect("parsing to succeed");
        let addr3 = Multiaddr::from_str("/ip4/127.0.0.3").expect("parsing to succeed");
        store.add_address(&peer, &addr1);
        store.add_address(&peer, &addr2);
        store.add_address(&peer, &addr3);
        assert_eq!(
            store
                .records
                .get(&peer)
                .expect("peer to be in the store")
                .addresses()
                .collect::<Vec<_>>(),
            vec![&addr3, &addr2, &addr1]
        );
        store.add_address(&peer, &addr1);
        assert!(
            store
                .records
                .get(&peer)
                .expect("peer to be in the store")
                .addresses()
                .collect::<Vec<_>>()
                == vec![&addr1, &addr3, &addr2]
        );
        store.add_address(&peer, &addr3);
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
            let addr_string = format!("/ip4/127.0.0.{i}");
            store.add_address(
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
    fn update_address_on_connect() {
        async fn expect_record_update(
            swarm: &mut Swarm<crate::Behaviour<MemoryStore>>,
            expected_peer: PeerId,
            expected_address: Option<&Multiaddr>,
        ) {
            match swarm.next_behaviour_event().await {
                Event::PeerAddressAdded {
                    peer_id,
                    address,
                    is_permanent,
                } => {
                    assert_eq!(peer_id, expected_peer);
                    assert!(expected_address.is_none_or(|a| *a == address));
                    assert!(!is_permanent)
                }
                ev => panic!("Unexpected event {ev:?}."),
            }
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
            let (mut listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            let swarm1_peer_id = *swarm1.local_peer_id();
            let swarm2_peer_id = *swarm2.local_peer_id();
            swarm2.connect(&mut swarm1).await;

            listen_addr.push(Protocol::P2p(swarm1_peer_id));
            expect_record_update(&mut swarm2, swarm1_peer_id, Some(&listen_addr)).await;
            assert!(swarm2
                .behaviour()
                .address_of_peer(&swarm1_peer_id)
                .expect("swarm should be connected and record about it should be created")
                .any(|addr| *addr == listen_addr));
            // Address from connection is not stored on the listener side.
            assert!(swarm1
                .behaviour()
                .address_of_peer(&swarm2_peer_id)
                .is_none());
            let (mut new_listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            tokio::spawn(swarm1.loop_on_next());
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
            new_listen_addr.push(Protocol::P2p(swarm1_peer_id));
            expect_record_update(&mut swarm2, swarm1_peer_id, Some(&new_listen_addr)).await;
            // The address in store will contain peer ID.
            let new_listen_addr = new_listen_addr
                .with_p2p(swarm1_peer_id)
                .expect("extend to succeed");
            assert_eq!(
                swarm2
                    .behaviour()
                    .address_of_peer(&swarm1_peer_id)
                    .expect("peer to exist")
                    .collect::<Vec<_>>(),
                vec![&new_listen_addr, &listen_addr]
            );
        })
    }

    #[test]
    fn identify_external_addr_report() {
        #[derive(NetworkBehaviour)]
        struct Behaviour {
            peer_store: crate::Behaviour<MemoryStore>,
            identify: identify::Behaviour,
        }
        async fn expect_record_update(
            swarm: &mut Swarm<Behaviour>,
            expected_peer: PeerId,
            expected_address: Option<&Multiaddr>,
        ) {
            loop {
                match swarm.next_behaviour_event().await {
                    BehaviourEvent::PeerStore(Event::PeerAddressAdded {
                        peer_id,
                        address,
                        is_permanent,
                    }) => {
                        assert_eq!(peer_id, expected_peer);
                        assert!(expected_address.is_none_or(|a| *a == address));
                        assert!(!is_permanent);
                        break;
                    }
                    ev @ BehaviourEvent::PeerStore(_) => panic!("Unexpected event {ev:?}."),
                    _ => {}
                }
            }
        }
        fn build_swarm() -> Swarm<Behaviour> {
            Swarm::new_ephemeral_tokio(|kp| Behaviour {
                peer_store: crate::Behaviour::new(MemoryStore::new(
                    crate::memory_store::Config::default()
                        .set_record_capacity(NonZero::new(4).expect("4 > 0")),
                )),
                identify: identify::Behaviour::new(identify::Config::new(
                    "/TODO/0.0.1".to_string(),
                    kp.public(),
                )),
            })
        }
        let mut swarm1 = build_swarm();
        let mut swarm2 = build_swarm();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let (mut listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            let swarm1_peer_id = *swarm1.local_peer_id();
            let swarm2_peer_id = *swarm2.local_peer_id();
            swarm2.connect(&mut swarm1).await;

            listen_addr.push(Protocol::P2p(swarm1_peer_id));
            expect_record_update(&mut swarm2, swarm1_peer_id, Some(&listen_addr)).await;

            assert_eq!(
                swarm2
                    .behaviour()
                    .peer_store
                    .address_of_peer(&swarm1_peer_id)
                    .expect("swarm should be connected and record about it should be created")
                    .collect::<Vec<_>>(),
                vec![&listen_addr]
            );

            assert!(matches!(
                swarm1.next_behaviour_event().await,
                BehaviourEvent::Identify(identify::Event::Sent { .. })
            ));
            assert!(matches!(
                swarm1.next_behaviour_event().await,
                BehaviourEvent::Identify(identify::Event::Received { .. })
            ));
            let (new_listen_addr, _) = swarm1.listen().with_memory_addr_external().await;
            swarm1.behaviour_mut().identify.push([swarm2_peer_id]);
            tokio::spawn(swarm1.loop_on_next());
            // Expecting 3 updates from Identify:
            // 2 pair of mem and tcp address for two calls to `<Swarm as SwarmExt>::listen()`
            // with one address already present through direct connection.
            // FLAKY: tcp addresses are not explicitly marked as external addresses.
            expect_record_update(&mut swarm2, swarm1_peer_id, None).await;
            expect_record_update(&mut swarm2, swarm1_peer_id, None).await;
            expect_record_update(&mut swarm2, swarm1_peer_id, None).await;
            // The address in store won't contain peer ID because it is from Identify.
            let known_listen_addresses = swarm2
                .behaviour()
                .peer_store
                .address_of_peer(&swarm1_peer_id)
                .expect("peer to exist")
                .collect::<Vec<_>>();
            assert!(known_listen_addresses.contains(&&new_listen_addr));
            assert_eq!(known_listen_addresses.len(), 4)
        })
    }
}
