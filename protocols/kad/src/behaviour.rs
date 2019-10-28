// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! Implementation of the `Kademlia` network behaviour.

mod test;

use crate::K_VALUE;
use crate::addresses::Addresses;
use crate::handler::{KademliaHandler, KademliaRequestId, KademliaHandlerEvent, KademliaHandlerIn};
use crate::jobs::*;
use crate::kbucket::{self, KBucketsTable, NodeStatus};
use crate::protocol::{KadConnectionType, KadPeer};
use crate::query::{Query, QueryId, QueryPool, QueryConfig, QueryPoolState};
use crate::record::{self, store::{self, RecordStore}, Record, ProviderRecord};
use fnv::{FnvHashMap, FnvHashSet};
use futures::prelude::*;
use libp2p_core::{ConnectedPoint, Multiaddr, PeerId};
use libp2p_swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters, ProtocolsHandler};
use log::{info, debug, warn};
use smallvec::SmallVec;
use std::{borrow::Cow, error, iter, marker::PhantomData, time::Duration};
use std::collections::VecDeque;
use std::num::NonZeroUsize;
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::Instant;

/// Network behaviour that handles Kademlia.
pub struct Kademlia<TSubstream, TStore> {
    /// The Kademlia routing table.
    kbuckets: KBucketsTable<kbucket::Key<PeerId>, Addresses>,

    /// An optional protocol name override to segregate DHTs in the network.
    protocol_name_override: Option<Cow<'static, [u8]>>,

    /// The currently active (i.e. in-progress) queries.
    queries: QueryPool<QueryInner>,

    /// The currently connected peers.
    ///
    /// This is a superset of the connected peers currently in the routing table.
    connected_peers: FnvHashSet<PeerId>,

    /// Periodic job for re-publication of provider records for keys
    /// provided by the local node.
    add_provider_job: Option<AddProviderJob>,

    /// Periodic job for (re-)replication and (re-)publishing of
    /// regular (value-)records.
    put_record_job: Option<PutRecordJob>,

    /// The TTL of regular (value-)records.
    record_ttl: Option<Duration>,

    /// The TTL of provider records.
    provider_record_ttl: Option<Duration>,

    /// Queued events to return when the behaviour is being polled.
    queued_events: VecDeque<NetworkBehaviourAction<KademliaHandlerIn<QueryId>, KademliaEvent>>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,

    /// The record storage.
    store: TStore,
}

/// The configuration for the `Kademlia` behaviour.
///
/// The configuration is consumed by [`Kademlia::new`].
#[derive(Debug, Clone)]
pub struct KademliaConfig {
    kbucket_pending_timeout: Duration,
    query_config: QueryConfig,
    protocol_name_override: Option<Cow<'static, [u8]>>,
    record_ttl: Option<Duration>,
    record_replication_interval: Option<Duration>,
    record_publication_interval: Option<Duration>,
    provider_record_ttl: Option<Duration>,
    provider_publication_interval: Option<Duration>,
}

impl Default for KademliaConfig {
    fn default() -> Self {
        KademliaConfig {
            kbucket_pending_timeout: Duration::from_secs(60),
            query_config: QueryConfig::default(),
            protocol_name_override: None,
            record_ttl: Some(Duration::from_secs(36 * 60 * 60)),
            record_replication_interval: Some(Duration::from_secs(60 * 60)),
            record_publication_interval: Some(Duration::from_secs(24 * 60 * 60)),
            provider_publication_interval: Some(Duration::from_secs(12 * 60 * 60)),
            provider_record_ttl: Some(Duration::from_secs(24 * 60 * 60)),
        }
    }
}

impl KademliaConfig {
    /// Sets a custom protocol name.
    ///
    /// Kademlia nodes only communicate with other nodes using the same protocol name. Using a
    /// custom name therefore allows to segregate the DHT from others, if that is desired.
    pub fn set_protocol_name(&mut self, name: impl Into<Cow<'static, [u8]>>) -> &mut Self {
        self.protocol_name_override = Some(name.into());
        self
    }

    /// Sets the timeout for a single query.
    ///
    /// > **Note**: A single query usually comprises at least as many requests
    /// > as the replication factor, i.e. this is not a request timeout.
    ///
    /// The default is 60 seconds.
    pub fn set_query_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.query_config.timeout = timeout;
        self
    }

    /// Sets the replication factor to use.
    ///
    /// The replication factor determines to how many closest peers
    /// a record is replicated. The default is [`K_VALUE`].
    pub fn set_replication_factor(&mut self, replication_factor: NonZeroUsize) -> &mut Self {
        self.query_config.replication_factor = replication_factor;
        self
    }

    /// Sets the TTL for stored records.
    ///
    /// The TTL should be significantly longer than the (re-)publication
    /// interval, to avoid premature expiration of records. The default is 36 hours.
    ///
    /// `None` means records never expire.
    ///
    /// Does not apply to provider records.
    pub fn set_record_ttl(&mut self, record_ttl: Option<Duration>) -> &mut Self {
        self.record_ttl = record_ttl;
        self
    }

    /// Sets the (re-)replication interval for stored records.
    ///
    /// Periodic replication of stored records ensures that the records
    /// are always replicated to the available nodes closest to the key in the
    /// context of DHT topology changes (i.e. nodes joining and leaving), thus
    /// ensuring persistence until the record expires. Replication does not
    /// prolong the regular lifetime of a record (for otherwise it would live
    /// forever regardless of the configured TTL). The expiry of a record
    /// is only extended through re-publication.
    ///
    /// This interval should be significantly shorter than the publication
    /// interval, to ensure persistence between re-publications. The default
    /// is 1 hour.
    ///
    /// `None` means that stored records are never re-replicated.
    ///
    /// Does not apply to provider records.
    pub fn set_replication_interval(&mut self, interval: Option<Duration>) -> &mut Self {
        self.record_replication_interval = interval;
        self
    }

    /// Sets the (re-)publication interval of stored records.
    ///
    /// Records persist in the DHT until they expire. By default, published records
    /// are re-published in regular intervals for as long as the record exists
    /// in the local storage of the original publisher, thereby extending the
    /// records lifetime.
    ///
    /// This interval should be significantly shorter than the record TTL, to
    /// ensure records do not expire prematurely. The default is 24 hours.
    ///
    /// `None` means that stored records are never automatically re-published.
    ///
    /// Does not apply to provider records.
    pub fn set_publication_interval(&mut self, interval: Option<Duration>) -> &mut Self {
        self.record_publication_interval = interval;
        self
    }

    /// Sets the TTL for provider records.
    ///
    /// `None` means that stored provider records never expire.
    ///
    /// Must be significantly larger than the provider publication interval.
    pub fn set_provider_record_ttl(&mut self, ttl: Option<Duration>) -> &mut Self {
        self.provider_record_ttl = ttl;
        self
    }

    /// Sets the interval at which provider records for keys provided
    /// by the local node are re-published.
    ///
    /// `None` means that stored provider records are never automatically re-published.
    ///
    /// Must be significantly less than the provider record TTL.
    pub fn set_provider_publication_interval(&mut self, interval: Option<Duration>) -> &mut Self {
        self.provider_publication_interval = interval;
        self
    }
}

impl<TSubstream, TStore> Kademlia<TSubstream, TStore>
where
    for<'a> TStore: RecordStore<'a>
{
    /// Creates a new `Kademlia` network behaviour with the given configuration.
    pub fn new(id: PeerId, store: TStore) -> Self {
        Self::with_config(id, store, Default::default())
    }

    /// Creates a new `Kademlia` network behaviour with the given configuration.
    pub fn with_config(id: PeerId, store: TStore, config: KademliaConfig) -> Self {
        let local_key = kbucket::Key::new(id.clone());

        let put_record_job = config
            .record_replication_interval
            .or(config.record_publication_interval)
            .map(|interval| PutRecordJob::new(
                id.clone(),
                interval,
                config.record_publication_interval,
                config.record_ttl,
            ));

        let add_provider_job = config
            .provider_publication_interval
            .map(AddProviderJob::new);

        Kademlia {
            store,
            kbuckets: KBucketsTable::new(local_key, config.kbucket_pending_timeout),
            protocol_name_override: config.protocol_name_override,
            queued_events: VecDeque::with_capacity(config.query_config.replication_factor.get()),
            queries: QueryPool::new(config.query_config),
            connected_peers: Default::default(),
            add_provider_job,
            put_record_job,
            record_ttl: config.record_ttl,
            provider_record_ttl: config.provider_record_ttl,
            marker: PhantomData,
        }
    }

    /// Adds a known listen address of a peer participating in the DHT to the
    /// routing table.
    ///
    /// Explicitly adding addresses of peers serves two purposes:
    ///
    ///   1. In order for a node to join the DHT, it must know about at least
    ///      one other node of the DHT.
    ///
    ///   2. When a remote peer initiates a connection and that peer is not
    ///      yet in the routing table, the `Kademlia` behaviour must be
    ///      informed of an address on which that peer is listening for
    ///      connections before it can be added to the routing table
    ///      from where it can subsequently be discovered by all peers
    ///      in the DHT.
    ///
    /// If the routing table has been updated as a result of this operation,
    /// a [`KademliaEvent::RoutingUpdated`] event is emitted.
    pub fn add_address(&mut self, peer: &PeerId, address: Multiaddr) {
        let key = kbucket::Key::new(peer.clone());
        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(mut entry, _) => {
                if entry.value().insert(address) {
                    self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                        KademliaEvent::RoutingUpdated {
                            peer: peer.clone(),
                            addresses: entry.value().clone(),
                            old_peer: None,
                        }
                    ))
                }
            }
            kbucket::Entry::Pending(mut entry, _) => {
                entry.value().insert(address);
            }
            kbucket::Entry::Absent(entry) => {
                let addresses = Addresses::new(address);
                let status =
                    if self.connected_peers.contains(peer) {
                        NodeStatus::Connected
                    } else {
                        NodeStatus::Disconnected
                    };
                match entry.insert(addresses.clone(), status) {
                    kbucket::InsertResult::Inserted => {
                        self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                            KademliaEvent::RoutingUpdated {
                                peer: peer.clone(),
                                addresses,
                                old_peer: None,
                            }
                        ));
                    },
                    kbucket::InsertResult::Full => {
                        debug!("Bucket full. Peer not added to routing table: {}", peer)
                    },
                    kbucket::InsertResult::Pending { disconnected } => {
                        self.queued_events.push_back(NetworkBehaviourAction::DialPeer {
                            peer_id: disconnected.into_preimage(),
                        })
                    },
                }
            },
            kbucket::Entry::SelfEntry => {},
        }
    }

    /// Returns an iterator over all peer IDs of nodes currently contained in a bucket
    /// of the Kademlia routing table.
    pub fn kbuckets_entries(&mut self) -> impl Iterator<Item = &PeerId> {
        self.kbuckets.iter().map(|entry| entry.node.key.preimage())
    }

    /// Performs a lookup for the closest peers to the given key.
    ///
    /// The result of this operation is delivered in [`KademliaEvent::GetClosestPeersResult`].
    pub fn get_closest_peers<K>(&mut self, key: K)
    where
        K: AsRef<[u8]> + Clone
    {
        let key = key.as_ref().to_vec();
        let info = QueryInfo::GetClosestPeers { key: key.clone() };
        let target = kbucket::Key::new(key);
        let peers = self.kbuckets.closest_keys(&target);
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Performs a lookup for a record in the DHT.
    ///
    /// The result of this operation is delivered in [`KademliaEvent::GetRecordResult`].
    pub fn get_record(&mut self, key: &record::Key, quorum: Quorum) {
        let quorum = quorum.eval(self.queries.config().replication_factor);
        let mut records = Vec::with_capacity(quorum.get());

        if let Some(record) = self.store.get(key) {
            if record.is_expired(Instant::now()) {
                self.store.remove(key)
            } else {
                records.push(record.into_owned());
                if quorum.get() == 1 {
                    self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                        KademliaEvent::GetRecordResult(Ok(GetRecordOk { records }))
                    ));
                    return;
                }
            }
        }

        let target = kbucket::Key::new(key.clone());
        let info = QueryInfo::GetRecord { key: key.clone(), records, quorum, cache_at: None };
        let peers = self.kbuckets.closest_keys(&target);
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Stores a record in the DHT.
    ///
    /// The result of this operation is delivered in [`KademliaEvent::PutRecordResult`].
    ///
    /// The record is always stored locally with the given expiration. If the record's
    /// expiration is `None`, the common case, it does not expire in local storage
    /// but is still replicated with the configured record TTL. To remove the record
    /// locally and stop it from being re-published in the DHT, see [`Kademlia::remove_record`].
    ///
    /// After the initial publication of the record, it is subject to (re-)replication
    /// and (re-)publication as per the configured intervals. Periodic (re-)publication
    /// does not update the record's expiration in local storage, thus a given record
    /// with an explicit expiration will always expire at that instant and until then
    /// is subject to regular (re-)replication and (re-)publication.
    pub fn put_record(&mut self, mut record: Record, quorum: Quorum) {
        record.publisher = Some(self.kbuckets.local_key().preimage().clone());
        if let Err(err) = self.store.put(record.clone()) {
            self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                KademliaEvent::PutRecordResult(Err(
                    PutRecordError::LocalStorageError {
                        key: record.key,
                        cause: err,
                    }
                ))
            ));
        } else {
            record.expires = record.expires.or_else(||
                self.record_ttl.map(|ttl| Instant::now() + ttl));
            let quorum = quorum.eval(self.queries.config().replication_factor);
            let target = kbucket::Key::new(record.key.clone());
            let peers = self.kbuckets.closest_keys(&target);
            let context = PutRecordContext::Publish;
            let info = QueryInfo::PreparePutRecord { record, quorum, context };
            let inner = QueryInner::new(info);
            self.queries.add_iter_closest(target.clone(), peers, inner);
        }
    }

    /// Removes the record with the given key from _local_ storage,
    /// if the local node is the publisher of the record.
    ///
    /// Has no effect if a record for the given key is stored locally but
    /// the local node is not a publisher of the record.
    ///
    /// This is a _local_ operation. However, it also has the effect that
    /// the record will no longer be periodically re-published, allowing the
    /// record to eventually expire throughout the DHT.
    pub fn remove_record(&mut self, key: &record::Key) {
        if let Some(r) = self.store.get(key) {
            if r.publisher.as_ref() == Some(self.kbuckets.local_key().preimage()) {
                self.store.remove(key)
            }
        }
    }

    /// Gets a mutable reference to the record store.
    pub fn store_mut(&mut self) -> &mut TStore {
        &mut self.store
    }

    /// Bootstraps the local node to join the DHT.
    ///
    /// Bootstrapping is a multi-step operation that starts with a lookup of the local node's
    /// own ID in the DHT. This introduces the local node to the other nodes
    /// in the DHT and populates its routing table with the closest neighbours.
    ///
    /// Subsequently, all buckets farther from the bucket of the closest neighbour are
    /// refreshed by initiating an additional bootstrapping query for each such
    /// bucket with random keys.
    ///
    /// The result(s) of this operation are delivered in [`KademliaEvent::BootstrapResult`],
    /// with one event per bootstrapping query.
    ///
    /// > **Note**: Bootstrapping requires at least one node of the DHT to be known.
    /// > See [`Kademlia::add_address`].
    pub fn bootstrap(&mut self) {
        let local_key = self.kbuckets.local_key().clone();
        let info = QueryInfo::Bootstrap { peer: local_key.preimage().clone() };
        let peers = self.kbuckets.closest_keys(&local_key).collect::<Vec<_>>();
        // TODO: Emit error if `peers` is empty? BootstrapError::NoPeers?
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(local_key, peers, inner);
    }

    /// Establishes the local node as a provider of a value for the given key.
    ///
    /// This operation publishes a provider record with the given key and
    /// identity of the local node to the peers closest to the key, thus establishing
    /// the local node as a provider.
    ///
    /// The publication of the provider records is periodically repeated as per the
    /// configured interval, to renew the expiry and account for changes to the DHT
    /// topology. A provider record may be removed from local storage and
    /// thus no longer re-published by calling [`Kademlia::stop_providing`].
    ///
    /// In contrast to the standard Kademlia push-based model for content distribution
    /// implemented by [`Kademlia::put_record`], the provider API implements a
    /// pull-based model that may be used in addition or as an alternative.
    /// The means by which the actual value is obtained from a provider is out of scope
    /// of the libp2p Kademlia provider API.
    ///
    /// The results of the (repeated) provider announcements sent by this node are
    /// delivered in [`KademliaEvent::AddProviderResult`].
    pub fn start_providing(&mut self, key: record::Key) {
        let record = ProviderRecord::new(key.clone(), self.kbuckets.local_key().preimage().clone());
        if let Err(err) = self.store.add_provider(record) {
            self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                KademliaEvent::StartProvidingResult(Err(
                    AddProviderError::LocalStorageError { key, cause: err }
                ))
            ));
        } else {
            let target = kbucket::Key::new(key.clone());
            let peers = self.kbuckets.closest_keys(&target);
            let context = AddProviderContext::Publish;
            let info = QueryInfo::PrepareAddProvider { key, context };
            let inner = QueryInner::new(info);
            self.queries.add_iter_closest(target.clone(), peers, inner);
        }
    }

    /// Stops the local node from announcing that it is a provider for the given key.
    ///
    /// This is a local operation. The local node will still be considered as a
    /// provider for the key by other nodes until these provider records expire.
    pub fn stop_providing(&mut self, key: &record::Key) {
        self.store.remove_provider(key, self.kbuckets.local_key().preimage());
    }

    /// Performs a lookup for providers of a value to the given key.
    ///
    /// The result of this operation is delivered in [`KademliaEvent::GetProvidersResult`].
    pub fn get_providers(&mut self, key: record::Key) {
        let info = QueryInfo::GetProviders {
            key: key.clone(),
            providers: Vec::new(),
        };
        let target = kbucket::Key::new(key);
        let peers = self.kbuckets.closest_keys(&target);
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Processes discovered peers from a successful request in an iterative `Query`.
    fn discovered<'a, I>(&'a mut self, query_id: &QueryId, source: &PeerId, peers: I)
    where
        I: Iterator<Item = &'a KadPeer> + Clone
    {
        let local_id = self.kbuckets.local_key().preimage().clone();
        let others_iter = peers.filter(|p| p.node_id != local_id);

        for peer in others_iter.clone() {
            self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                KademliaEvent::Discovered {
                    peer_id: peer.node_id.clone(),
                    addresses: peer.multiaddrs.clone(),
                    ty: peer.connection_ty,
                }
            ));
        }

        if let Some(query) = self.queries.get_mut(query_id) {
            for peer in others_iter.clone() {
                query.inner.addresses
                    .insert(peer.node_id.clone(), peer.multiaddrs.iter().cloned().collect());
            }
            query.on_success(source, others_iter.cloned().map(|kp| kp.node_id))
        }
    }

    /// Finds the closest peers to a `target` in the context of a request by
    /// the `source` peer, such that the `source` peer is never included in the
    /// result.
    fn find_closest<T: Clone>(&mut self, target: &kbucket::Key<T>, source: &PeerId) -> Vec<KadPeer> {
        if target == self.kbuckets.local_key() {
            Vec::new()
        } else {
            self.kbuckets
                .closest(target)
                .filter(|e| e.node.key.preimage() != source)
                .take(self.queries.config().replication_factor.get())
                .map(KadPeer::from)
                .collect()
        }
    }

    /// Collects all peers who are known to be providers of the value for a given `Multihash`.
    fn provider_peers(&mut self, key: &record::Key, source: &PeerId) -> Vec<KadPeer> {
        let kbuckets = &mut self.kbuckets;
        self.store.providers(key)
            .into_iter()
            .filter_map(move |p|
                if &p.provider != source {
                    let key = kbucket::Key::new(p.provider.clone());
                    kbuckets.entry(&key).view().map(|e| KadPeer::from(e.to_owned()))
                } else {
                    None
                })
            .take(self.queries.config().replication_factor.get())
            .collect()
    }

    /// Starts an iterative `ADD_PROVIDER` query for the given key.
    fn start_add_provider(&mut self, key: record::Key, context: AddProviderContext) {
        let info = QueryInfo::PrepareAddProvider { key: key.clone(), context };
        let target = kbucket::Key::new(key);
        let peers = self.kbuckets.closest_keys(&target);
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Starts an iterative `PUT_VALUE` query for the given record.
    fn start_put_record(&mut self, record: Record, quorum: Quorum, context: PutRecordContext) {
        let quorum = quorum.eval(self.queries.config().replication_factor);
        let target = kbucket::Key::new(record.key.clone());
        let peers = self.kbuckets.closest_keys(&target);
        let info = QueryInfo::PreparePutRecord { record, quorum, context };
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Updates the connection status of a peer in the Kademlia routing table.
    fn connection_updated(&mut self, peer: PeerId, address: Option<Multiaddr>, new_status: NodeStatus) {
        let key = kbucket::Key::new(peer.clone());
        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(mut entry, old_status) => {
                if let Some(address) = address {
                    if entry.value().insert(address) {
                        self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                            KademliaEvent::RoutingUpdated {
                                peer,
                                addresses: entry.value().clone(),
                                old_peer: None,
                            }
                        ))
                    }
                }
                if old_status != new_status {
                    entry.update(new_status);
                }
            },

            kbucket::Entry::Pending(mut entry, old_status) => {
                if let Some(address) = address {
                    entry.value().insert(address);
                }
                if old_status != new_status {
                    entry.update(new_status);
                }
            },

            kbucket::Entry::Absent(entry) => {
                // Only connected nodes with a known address are newly inserted.
                if new_status == NodeStatus::Connected {
                    if let Some(address) = address {
                        let addresses = Addresses::new(address);
                        match entry.insert(addresses.clone(), new_status) {
                            kbucket::InsertResult::Inserted => {
                                let event = KademliaEvent::RoutingUpdated {
                                    peer: peer.clone(),
                                    addresses,
                                    old_peer: None,
                                };
                                self.queued_events.push_back(
                                    NetworkBehaviourAction::GenerateEvent(event));
                            },
                            kbucket::InsertResult::Full => {
                                debug!("Bucket full. Peer not added to routing table: {}", peer)
                            },
                            kbucket::InsertResult::Pending { disconnected } => {
                                debug_assert!(!self.connected_peers.contains(disconnected.preimage()));
                                self.queued_events.push_back(NetworkBehaviourAction::DialPeer {
                                    peer_id: disconnected.into_preimage(),
                                })
                            },
                        }
                    } else {
                        self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                            KademliaEvent::UnroutablePeer { peer }
                        ));
                    }
                }
            },
            _ => {}
        }
    }

    /// Handles a finished (i.e. successful) query.
    fn query_finished(&mut self, q: Query<QueryInner>, params: &mut impl PollParameters)
        -> Option<KademliaEvent>
    {
        let result = q.into_result();
        match result.inner.info {
            QueryInfo::Bootstrap { peer } => {
                let local_key = self.kbuckets.local_key().clone();
                if &peer == local_key.preimage() {
                    // The lookup for the local key finished. To complete the bootstrap process,
                    // a bucket refresh should be performed for every bucket farther away than
                    // the first non-empty bucket (which are most likely no more than the last
                    // few, i.e. farthest, buckets).
                    let targets = self.kbuckets.buckets()
                        .skip_while(|b| b.num_entries() == 0)
                        .skip(1) // Skip the bucket with the closest neighbour.
                        .map(|b| {
                            // Try to find a key that falls into the bucket. While such keys can
                            // be generated fully deterministically, the current libp2p kademlia
                            // wire protocol requires transmission of the preimages of the actual
                            // keys in the DHT keyspace, hence for now this is just a "best effort"
                            // to find a key that hashes into a specific bucket. The probabilities
                            // of finding a key in the bucket `b` with as most 16 trials are as
                            // follows:
                            //
                            // Pr(bucket-255) = 1 - (1/2)^16   ~= 1
                            // Pr(bucket-254) = 1 - (3/4)^16   ~= 1
                            // Pr(bucket-253) = 1 - (7/8)^16   ~= 0.88
                            // Pr(bucket-252) = 1 - (15/16)^16 ~= 0.64
                            // ...
                            let mut target = kbucket::Key::new(PeerId::random());
                            for _ in 0 .. 16 {
                                let d = local_key.distance(&target);
                                if b.contains(&d) {
                                    break;
                                }
                                target = kbucket::Key::new(PeerId::random());
                            }
                            target
                        }).collect::<Vec<_>>();

                    for target in targets {
                        let info = QueryInfo::Bootstrap { peer: target.clone().into_preimage() };
                        let peers = self.kbuckets.closest_keys(&target);
                        let inner = QueryInner::new(info);
                        self.queries.add_iter_closest(target.clone(), peers, inner);
                    }
                }
                Some(KademliaEvent::BootstrapResult(Ok(BootstrapOk { peer })))
            }

            QueryInfo::GetClosestPeers { key, .. } => {
                Some(KademliaEvent::GetClosestPeersResult(Ok(
                    GetClosestPeersOk { key, peers: result.peers.collect() }
                )))
            }

            QueryInfo::GetProviders { key, providers } => {
                Some(KademliaEvent::GetProvidersResult(Ok(
                    GetProvidersOk {
                        key,
                        providers,
                        closest_peers: result.peers.collect()
                    }
                )))
            }

            QueryInfo::PrepareAddProvider { key, context } => {
                let closest_peers = result.peers.map(kbucket::Key::from);
                let provider_id = params.local_peer_id().clone();
                let external_addresses = params.external_addresses().collect();
                let inner = QueryInner::new(QueryInfo::AddProvider {
                    key,
                    provider_id,
                    external_addresses,
                    context,
                });
                self.queries.add_fixed(closest_peers, inner);
                None
            }

            QueryInfo::AddProvider { key, context, .. } => {
                match context {
                    AddProviderContext::Publish => {
                        Some(KademliaEvent::StartProvidingResult(Ok(
                            AddProviderOk { key }
                        )))
                    }
                    AddProviderContext::Republish => {
                        Some(KademliaEvent::RepublishProviderResult(Ok(
                            AddProviderOk { key }
                        )))
                    }
                }
            }

            QueryInfo::GetRecord { key, records, quorum, cache_at } => {
                let result = if records.len() >= quorum.get() { // [not empty]
                    if let Some(cache_key) = cache_at {
                        // Cache the record at the closest node to the key that
                        // did not return the record.
                        let record = records.first().expect("[not empty]").clone();
                        let quorum = NonZeroUsize::new(1).expect("1 > 0");
                        let context = PutRecordContext::Cache;
                        let info = QueryInfo::PutRecord { record, quorum, context, num_results: 0 };
                        let inner = QueryInner::new(info);
                        self.queries.add_fixed(iter::once(cache_key), inner);
                    }
                    Ok(GetRecordOk { records })
                } else if records.is_empty() {
                    Err(GetRecordError::NotFound {
                        key,
                        closest_peers: result.peers.collect()
                    })
                } else {
                    Err(GetRecordError::QuorumFailed { key, records, quorum })
                };
                Some(KademliaEvent::GetRecordResult(result))
            }

            QueryInfo::PreparePutRecord { record, quorum, context } => {
                let closest_peers = result.peers.map(kbucket::Key::from);
                let info = QueryInfo::PutRecord { record, quorum, context, num_results: 0 };
                let inner = QueryInner::new(info);
                self.queries.add_fixed(closest_peers, inner);
                None
            }

            QueryInfo::PutRecord { record, quorum, num_results, context } => {
                let result = |key: record::Key| {
                    if num_results >= quorum.get() {
                        Ok(PutRecordOk { key })
                    } else {
                        Err(PutRecordError::QuorumFailed { key, quorum, num_results })
                    }
                };
                match context {
                    PutRecordContext::Publish =>
                        Some(KademliaEvent::PutRecordResult(result(record.key))),
                    PutRecordContext::Republish =>
                        Some(KademliaEvent::RepublishRecordResult(result(record.key))),
                    PutRecordContext::Replicate => {
                        debug!("Record replicated: {:?}", record.key);
                        None
                    }
                    PutRecordContext::Cache => {
                        debug!("Record cached: {:?}", record.key);
                        None
                    }
                }
            }
        }
    }

    /// Handles a query that timed out.
    fn query_timeout(&self, query: Query<QueryInner>) -> Option<KademliaEvent> {
        let result = query.into_result();
        match result.inner.info {
            QueryInfo::Bootstrap { peer } =>
                Some(KademliaEvent::BootstrapResult(Err(
                    BootstrapError::Timeout { peer }))),

            QueryInfo::PrepareAddProvider { key, context } =>
                Some(match context {
                    AddProviderContext::Publish =>
                        KademliaEvent::StartProvidingResult(Err(
                            AddProviderError::Timeout { key })),
                    AddProviderContext::Republish =>
                        KademliaEvent::RepublishProviderResult(Err(
                            AddProviderError::Timeout { key })),
                }),

            QueryInfo::AddProvider { key, context, .. } =>
                Some(match context {
                    AddProviderContext::Publish =>
                        KademliaEvent::StartProvidingResult(Err(
                            AddProviderError::Timeout { key })),
                    AddProviderContext::Republish =>
                        KademliaEvent::RepublishProviderResult(Err(
                            AddProviderError::Timeout { key })),
                }),

            QueryInfo::GetClosestPeers { key } =>
                Some(KademliaEvent::GetClosestPeersResult(Err(
                    GetClosestPeersError::Timeout {
                        key,
                        peers: result.peers.collect()
                    }))),

            QueryInfo::PreparePutRecord { record, quorum, context, .. } => {
                let err = Err(PutRecordError::Timeout {
                    key: record.key,
                    num_results: 0,
                    quorum
                });
                match context {
                    PutRecordContext::Publish =>
                        Some(KademliaEvent::PutRecordResult(err)),
                    PutRecordContext::Republish =>
                        Some(KademliaEvent::RepublishRecordResult(err)),
                    PutRecordContext::Replicate => {
                        warn!("Locating closest peers for replication failed: {:?}", err);
                        None
                    }
                    PutRecordContext::Cache =>
                        // Caching a record at the closest peer to a key that did not return
                        // a record is never preceded by a lookup for the closest peers, i.e.
                        // it is a direct query to a single peer.
                        unreachable!()
                }
            }

            QueryInfo::PutRecord { record, quorum, num_results, context } => {
                let err = Err(PutRecordError::Timeout {
                    key: record.key,
                    num_results,
                    quorum
                });
                match context {
                    PutRecordContext::Publish =>
                        Some(KademliaEvent::PutRecordResult(err)),
                    PutRecordContext::Republish =>
                        Some(KademliaEvent::RepublishRecordResult(err)),
                    PutRecordContext::Replicate => {
                        debug!("Replicatiing record failed: {:?}", err);
                        None
                    }
                    PutRecordContext::Cache => {
                        debug!("Caching record failed: {:?}", err);
                        None
                    }
                }
            }

            QueryInfo::GetRecord { key, records, quorum, .. } =>
                Some(KademliaEvent::GetRecordResult(Err(
                    GetRecordError::Timeout { key, records, quorum }))),

            QueryInfo::GetProviders { key, providers } =>
                Some(KademliaEvent::GetProvidersResult(Err(
                    GetProvidersError::Timeout {
                        key,
                        providers,
                        closest_peers: result.peers.collect()
                    }))),
        }
    }

    /// Processes a record received from a peer.
    fn record_received(&mut self, source: PeerId, request_id: KademliaRequestId, mut record: Record) {
        if record.publisher.as_ref() == Some(self.kbuckets.local_key().preimage()) {
            // If the (alleged) publisher is the local node, do nothing. The record of
            // the original publisher should never change as a result of replication
            // and the publisher is always assumed to have the "right" value.
            self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: source,
                event: KademliaHandlerIn::PutRecordRes {
                    key: record.key,
                    value: record.value,
                    request_id,
                },
            });
            return
        }

        let now = Instant::now();

        // Calculate the expiration exponentially inversely proportional to the
        // number of nodes between the local node and the closest node to the key
        // (beyond the replication factor). This ensures avoiding over-caching
        // outside of the k closest nodes to a key.
        let target = kbucket::Key::new(record.key.clone());
        let num_between = self.kbuckets.count_nodes_between(&target);
        let k = self.queries.config().replication_factor.get();
        let num_beyond_k = (usize::max(k, num_between) - k) as u32;
        let expiration = self.record_ttl.map(|ttl|
            now + Duration::from_secs(ttl.as_secs() >> num_beyond_k)
        );
        // The smaller TTL prevails. Only if neither TTL is set is the record
        // stored "forever".
        record.expires = record.expires.or(expiration).min(expiration);

        if let Some(job) = self.put_record_job.as_mut() {
            // Ignore the record in the next run of the replication
            // job, since we can assume the sender replicated the
            // record to the k closest peers. Effectively, only
            // one of the k closest peers performs a replication
            // in the configured interval, assuming a shared interval.
            job.skip(record.key.clone())
        }

        // While records received from a publisher, as well as records that do
        // not exist locally should always (attempted to) be stored, there is a
        // choice here w.r.t. the handling of replicated records whose keys refer
        // to records that exist locally: The value and / or the publisher may
        // either be overridden or left unchanged. At the moment and in the
        // absence of a decisive argument for another option, both are always
        // overridden as it avoids having to load the existing record in the
        // first place.

        // The record is cloned because of the weird libp2p protocol requirement
        // to send back the value in the response, although this is a waste of
        // resources.
        match self.store.put(record.clone()) {
            Ok(()) => {
                debug!("Record stored: {:?}; {} bytes", record.key, record.value.len());
                self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: source,
                    event: KademliaHandlerIn::PutRecordRes {
                        key: record.key,
                        value: record.value,
                        request_id,
                    },
                })
            }
            Err(e) => {
                info!("Record not stored: {:?}", e);
                self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: source,
                    event: KademliaHandlerIn::Reset(request_id)
                })
            }
        }
    }

    /// Processes a provider record received from a peer.
    fn provider_received(&mut self, key: record::Key, provider: KadPeer) {
        self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
            KademliaEvent::Discovered {
                peer_id: provider.node_id.clone(),
                addresses: provider.multiaddrs.clone(),
                ty: provider.connection_ty,
            }));

        if &provider.node_id != self.kbuckets.local_key().preimage() {
            let record = ProviderRecord {
                key,
                provider: provider.node_id,
                expires: self.provider_record_ttl.map(|ttl| Instant::now() + ttl)
            };
            if let Err(e) = self.store.add_provider(record) {
                info!("Provider record not stored: {:?}", e);
            }
        }
    }
}

impl<TSubstream, TStore> NetworkBehaviour for Kademlia<TSubstream, TStore>
where
    TSubstream: AsyncRead + AsyncWrite,
    for<'a> TStore: RecordStore<'a>,
{
    type ProtocolsHandler = KademliaHandler<TSubstream, QueryId>;
    type OutEvent = KademliaEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        let mut handler = KademliaHandler::dial_and_listen();
        if let Some(name) = self.protocol_name_override.as_ref() {
            handler = handler.with_protocol_name(name.clone());
        }
        handler
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        // We should order addresses from decreasing likelyhood of connectivity, so start with
        // the addresses of that peer in the k-buckets.
        let key = kbucket::Key::new(peer_id.clone());
        let mut peer_addrs =
            if let kbucket::Entry::Present(mut entry, _) = self.kbuckets.entry(&key) {
                let addrs = entry.value().iter().cloned().collect::<Vec<_>>();
                debug_assert!(!addrs.is_empty(), "Empty peer addresses in routing table.");
                addrs
            } else {
                Vec::new()
            };

        // We add to that a temporary list of addresses from the ongoing queries.
        for query in self.queries.iter() {
            if let Some(addrs) = query.inner.addresses.get(peer_id) {
                peer_addrs.extend(addrs.iter().cloned())
            }
        }

        peer_addrs
    }

    fn inject_connected(&mut self, peer: PeerId, endpoint: ConnectedPoint) {
        // Queue events for sending pending RPCs to the connected peer.
        // There can be only one pending RPC for a particular peer and query per definition.
        for (peer_id, event) in self.queries.iter_mut().filter_map(|q|
            q.inner.pending_rpcs.iter()
                .position(|(p, _)| p == &peer)
                .map(|p| q.inner.pending_rpcs.remove(p)))
        {
            self.queued_events.push_back(NetworkBehaviourAction::SendEvent { peer_id, event });
        }

        // The remote's address can only be put into the routing table,
        // and thus shared with other nodes, if the local node is the dialer,
        // since the remote address on an inbound connection is specific to
        // that connection (e.g. typically the TCP port numbers).
        let address = match endpoint {
            ConnectedPoint::Dialer { address } => Some(address),
            ConnectedPoint::Listener { .. } => None,
        };

        self.connection_updated(peer.clone(), address, NodeStatus::Connected);
        self.connected_peers.insert(peer);
    }

    fn inject_addr_reach_failure(
        &mut self,
        peer_id: Option<&PeerId>,
        addr: &Multiaddr,
        err: &dyn error::Error
    ) {
        if let Some(peer_id) = peer_id {
            let key = kbucket::Key::new(peer_id.clone());

            if let Some(addrs) = self.kbuckets.entry(&key).value() {
                // TODO: Ideally, the address should only be removed if the error can
                // be classified as "permanent" but since `err` is currently a borrowed
                // trait object without a `'static` bound, even downcasting for inspection
                // of the error is not possible (and also not truly desirable or ergonomic).
                // The error passed in should rather be a dedicated enum.
                if addrs.remove(addr).is_ok() {
                    debug!("Address '{}' removed from peer '{}' due to error: {}.",
                        addr, peer_id, err);
                } else {
                    // Despite apparently having no reachable address (any longer),
                    // the peer is kept in the routing table with the last address to avoid
                    // (temporary) loss of network connectivity to "flush" the routing
                    // table. Once in, a peer is only removed from the routing table
                    // if it is the least recently connected peer, currently disconnected
                    // and is unreachable in the context of another peer pending insertion
                    // into the same bucket. This is handled transparently by the
                    // `KBucketsTable` and takes effect through `KBucketsTable::take_applied_pending`
                    // within `Kademlia::poll`.
                    debug!("Last remaining address '{}' of peer '{}' is unreachable: {}.",
                        addr, peer_id, err)
                }
            }

            for query in self.queries.iter_mut() {
                if let Some(addrs) = query.inner.addresses.get_mut(&peer_id) {
                    addrs.retain(|a| a != addr);
                }
            }
        }
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        for query in self.queries.iter_mut() {
            query.on_failure(peer_id);
        }
    }

    fn inject_disconnected(&mut self, id: &PeerId, _old_endpoint: ConnectedPoint) {
        for query in self.queries.iter_mut() {
            query.on_failure(id);
        }
        self.connection_updated(id.clone(), None, NodeStatus::Disconnected);
        self.connected_peers.remove(id);
    }

    fn inject_replaced(&mut self, peer_id: PeerId, _old: ConnectedPoint, new_endpoint: ConnectedPoint) {
        // We need to re-send the active queries.
        for query in self.queries.iter() {
            if query.is_waiting(&peer_id) {
                self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: peer_id.clone(),
                    event: query.inner.info.to_request(query.id()),
                });
            }
        }

        if let Some(addrs) = self.kbuckets.entry(&kbucket::Key::new(peer_id)).value() {
            if let ConnectedPoint::Dialer { address } = new_endpoint {
                addrs.insert(address);
            }
        }
    }

    fn inject_node_event(&mut self, source: PeerId, event: KademliaHandlerEvent<QueryId>) {
        match event {
            KademliaHandlerEvent::FindNodeReq { key, request_id } => {
                let closer_peers = self.find_closest(&kbucket::Key::new(key), &source);
                self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: source,
                    event: KademliaHandlerIn::FindNodeRes {
                        closer_peers,
                        request_id,
                    },
                });
            }

            KademliaHandlerEvent::FindNodeRes {
                closer_peers,
                user_data,
            } => {
                self.discovered(&user_data, &source, closer_peers.iter());
            }

            KademliaHandlerEvent::GetProvidersReq { key, request_id } => {
                let provider_peers = self.provider_peers(&key, &source);
                let closer_peers = self.find_closest(&kbucket::Key::new(key), &source);
                self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: source,
                    event: KademliaHandlerIn::GetProvidersRes {
                        closer_peers,
                        provider_peers,
                        request_id,
                    },
                });
            }

            KademliaHandlerEvent::GetProvidersRes {
                closer_peers,
                provider_peers,
                user_data,
            } => {
                let peers = closer_peers.iter().chain(provider_peers.iter());
                self.discovered(&user_data, &source, peers);
                if let Some(query) = self.queries.get_mut(&user_data) {
                    if let QueryInfo::GetProviders {
                        providers, ..
                    } = &mut query.inner.info {
                        for peer in provider_peers {
                            providers.push(peer.node_id);
                        }
                    }
                }
            }

            KademliaHandlerEvent::QueryError { user_data, .. } => {
                // It is possible that we obtain a response for a query that has finished, which is
                // why we may not find an entry in `self.queries`.
                if let Some(query) = self.queries.get_mut(&user_data) {
                    query.on_failure(&source)
                }
            }

            KademliaHandlerEvent::AddProvider { key, provider } => {
                // Only accept a provider record from a legitimate peer.
                if provider.node_id != source {
                    return
                }

                self.provider_received(key, provider)
            }

            KademliaHandlerEvent::GetRecord { key, request_id } => {
                // Lookup the record locally.
                let record = match self.store.get(&key) {
                    Some(record) => {
                        if record.is_expired(Instant::now()) {
                            self.store.remove(&key);
                            None
                        } else {
                            Some(record.into_owned())
                        }
                    },
                    None => None
                };

                // If no record is found, at least report known closer peers.
                let closer_peers =
                    if record.is_none() {
                        self.find_closest(&kbucket::Key::new(key), &source)
                    } else {
                        Vec::new()
                    };

                self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: source,
                    event: KademliaHandlerIn::GetRecordRes {
                        record,
                        closer_peers,
                        request_id,
                    },
                });
            }

            KademliaHandlerEvent::GetRecordRes {
                record,
                closer_peers,
                user_data,
            } => {
                if let Some(query) = self.queries.get_mut(&user_data) {
                    if let QueryInfo::GetRecord {
                        key, records, quorum, cache_at
                    } = &mut query.inner.info {
                        if let Some(record) = record {
                            records.push(record);
                            if records.len() == quorum.get() {
                                query.finish()
                            }
                        } else if quorum.get() == 1 {
                            // It is a "standard" Kademlia query, for which the
                            // closest node to the key that did *not* return the
                            // value is tracked in order to cache the record on
                            // that node if the query turns out to be successful.
                            let source_key = kbucket::Key::from(source.clone());
                            if let Some(cache_key) = cache_at {
                                let key = kbucket::Key::new(key.clone());
                                if source_key.distance(&key) < cache_key.distance(&key) {
                                    *cache_at = Some(source_key)
                                }
                            } else {
                                *cache_at = Some(source_key)
                            }
                        }
                    }
                }

                self.discovered(&user_data, &source, closer_peers.iter());
            }

            KademliaHandlerEvent::PutRecord {
                record,
                request_id
            } => {
                self.record_received(source, request_id, record);
            }

            KademliaHandlerEvent::PutRecordRes {
                user_data, ..
            } => {
                if let Some(query) = self.queries.get_mut(&user_data) {
                    query.on_success(&source, vec![]);
                    if let QueryInfo::PutRecord {
                        num_results, quorum, ..
                    } = &mut query.inner.info {
                        *num_results += 1;
                        if *num_results == quorum.get() {
                            query.finish()
                        }
                    }
                }
            }
        };
    }

    fn poll(&mut self, parameters: &mut impl PollParameters) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        let now = Instant::now();

        // Calculate the available capacity for queries triggered by background jobs.
        let mut jobs_query_capacity = JOBS_MAX_QUERIES.saturating_sub(self.queries.size());

        // Run the periodic provider announcement job.
        if let Some(mut job) = self.add_provider_job.take() {
            let num = usize::min(JOBS_MAX_NEW_QUERIES, jobs_query_capacity);
            for _ in 0 .. num {
                if let Async::Ready(r) = job.poll(&mut self.store, now) {
                    self.start_add_provider(r.key, AddProviderContext::Republish)
                } else {
                    break
                }
            }
            jobs_query_capacity -= num;
            self.add_provider_job = Some(job);
        }

        // Run the periodic record replication / publication job.
        if let Some(mut job) = self.put_record_job.take() {
            let num = usize::min(JOBS_MAX_NEW_QUERIES, jobs_query_capacity);
            for _ in 0 .. num {
                if let Async::Ready(r) = job.poll(&mut self.store, now) {
                    let context = if r.publisher.as_ref() == Some(self.kbuckets.local_key().preimage()) {
                        PutRecordContext::Republish
                    } else {
                        PutRecordContext::Replicate
                    };
                    self.start_put_record(r, Quorum::All, context)
                } else {
                    break
                }
            }
            self.put_record_job = Some(job);
        }

        loop {
            // Drain queued events first.
            if let Some(event) = self.queued_events.pop_front() {
                return Async::Ready(event);
            }

            // Drain applied pending entries from the routing table.
            if let Some(entry) = self.kbuckets.take_applied_pending() {
                let kbucket::Node { key, value } = entry.inserted;
                let event = KademliaEvent::RoutingUpdated {
                    peer: key.into_preimage(),
                    addresses: value,
                    old_peer: entry.evicted.map(|n| n.key.into_preimage())
                };
                return Async::Ready(NetworkBehaviourAction::GenerateEvent(event))
            }

            // Look for a finished query.
            loop {
                match self.queries.poll(now) {
                    QueryPoolState::Finished(q) => {
                        if let Some(event) = self.query_finished(q, parameters) {
                            return Async::Ready(NetworkBehaviourAction::GenerateEvent(event))
                        }
                    }
                    QueryPoolState::Timeout(q) => {
                        if let Some(event) = self.query_timeout(q) {
                            return Async::Ready(NetworkBehaviourAction::GenerateEvent(event))
                        }
                    }
                    QueryPoolState::Waiting(Some((query, peer_id))) => {
                        let event = query.inner.info.to_request(query.id());
                        // TODO: AddProvider requests yield no response, so the query completes
                        // as soon as all requests have been sent. However, the handler should
                        // better emit an event when the request has been sent (and report
                        // an error if sending fails), instead of immediately reporting
                        // "success" somewhat prematurely here.
                        if let QueryInfo::AddProvider { .. } = &query.inner.info {
                            query.on_success(&peer_id, vec![])
                        }
                        if self.connected_peers.contains(&peer_id) {
                            self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                                peer_id, event
                            });
                        } else if &peer_id != self.kbuckets.local_key().preimage() {
                            query.inner.pending_rpcs.push((peer_id.clone(), event));
                            self.queued_events.push_back(NetworkBehaviourAction::DialPeer {
                                peer_id
                            });
                        }
                    }
                    QueryPoolState::Waiting(None) | QueryPoolState::Idle => break,
                }
            }

            // No immediate event was produced as a result of a finished query.
            // If no new events have been queued either, signal `NotReady` to
            // be polled again later.
            if self.queued_events.is_empty() {
                return Async::NotReady
            }
        }
    }
}

/// A quorum w.r.t. the configured replication factor specifies the minimum
/// number of distinct nodes that must be successfully contacted in order
/// for a query to succeed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Quorum {
    One,
    Majority,
    All,
    N(NonZeroUsize)
}

impl Quorum {
    /// Evaluate the quorum w.r.t a given total (number of peers).
    fn eval(&self, total: NonZeroUsize) -> NonZeroUsize {
        match self {
            Quorum::One => NonZeroUsize::new(1).expect("1 != 0"),
            Quorum::Majority => NonZeroUsize::new(total.get() / 2 + 1).expect("n + 1 != 0"),
            Quorum::All => total,
            Quorum::N(n) => NonZeroUsize::min(total, *n)
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Events

/// The events produced by the `Kademlia` behaviour.
///
/// See [`Kademlia::poll`].
#[derive(Debug)]
pub enum KademliaEvent {
    /// The result of [`Kademlia::bootstrap`].
    BootstrapResult(BootstrapResult),

    /// The result of [`Kademlia::get_closest_peers`].
    GetClosestPeersResult(GetClosestPeersResult),

    /// The result of [`Kademlia::get_providers`].
    GetProvidersResult(GetProvidersResult),

    /// The result of [`Kademlia::start_providing`].
    StartProvidingResult(AddProviderResult),

    /// The result of a (automatic) republishing of a provider record.
    RepublishProviderResult(AddProviderResult),

    /// The result of [`Kademlia::get_record`].
    GetRecordResult(GetRecordResult),

    /// The result of [`Kademlia::put_record`].
    PutRecordResult(PutRecordResult),

    /// The result of a (automatic) republishing of a (value-)record.
    RepublishRecordResult(PutRecordResult),

    /// A peer has been discovered during a query.
    Discovered {
        /// The ID of the discovered peer.
        peer_id: PeerId,
        /// The known addresses of the discovered peer.
        addresses: Vec<Multiaddr>,
        /// The connection status reported by the discovered peer
        /// towards the local peer.
        ty: KadConnectionType,
    },

    /// The routing table has been updated.
    RoutingUpdated {
        /// The ID of the peer that was added or updated.
        peer: PeerId,
        /// The list of known addresses of `peer`.
        addresses: Addresses,
        /// The ID of the peer that was evicted from the routing table to make
        /// room for the new peer, if any.
        old_peer: Option<PeerId>,
    },

    /// A peer has connected for whom no listen address is known.
    ///
    /// If the peer is to be added to the local node's routing table, a known
    /// listen address for the peer must be provided via [`Kademlia::add_address`].
    UnroutablePeer {
        peer: PeerId
    }
}

/// The result of [`Kademlia::get_record`].
pub type GetRecordResult = Result<GetRecordOk, GetRecordError>;

/// The successful result of [`Kademlia::get_record`].
#[derive(Debug, Clone)]
pub struct GetRecordOk {
    pub records: Vec<Record>
}

/// The error result of [`Kademlia::get_record`].
#[derive(Debug, Clone)]
pub enum GetRecordError {
    NotFound {
        key: record::Key,
        closest_peers: Vec<PeerId>
    },
    QuorumFailed {
        key: record::Key,
        records: Vec<Record>,
        quorum: NonZeroUsize
    },
    Timeout {
        key: record::Key,
        records: Vec<Record>,
        quorum: NonZeroUsize
    }
}

impl GetRecordError {
    /// Gets the key of the record for which the operation failed.
    pub fn key(&self) -> &record::Key {
        match self {
            GetRecordError::QuorumFailed { key, .. } => key,
            GetRecordError::Timeout { key, .. } => key,
            GetRecordError::NotFound { key, .. } => key,
        }
    }

    /// Extracts the key of the record for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> record::Key {
        match self {
            GetRecordError::QuorumFailed { key, .. } => key,
            GetRecordError::Timeout { key, .. } => key,
            GetRecordError::NotFound { key, .. } => key,
        }
    }
}

/// The result of [`Kademlia::put_record`].
pub type PutRecordResult = Result<PutRecordOk, PutRecordError>;

/// The successful result of [`Kademlia::put_record`].
#[derive(Debug, Clone)]
pub struct PutRecordOk {
    pub key: record::Key
}

/// The error result of [`Kademlia::put_record`].
#[derive(Debug)]
pub enum PutRecordError {
    QuorumFailed {
        key: record::Key,
        num_results: usize,
        quorum: NonZeroUsize
    },
    Timeout {
        key: record::Key,
        num_results: usize,
        quorum: NonZeroUsize
    },
    LocalStorageError {
        key: record::Key,
        cause: store::Error
    }
}

impl PutRecordError {
    /// Gets the key of the record for which the operation failed.
    pub fn key(&self) -> &record::Key {
        match self {
            PutRecordError::QuorumFailed { key, .. } => key,
            PutRecordError::Timeout { key, .. } => key,
            PutRecordError::LocalStorageError { key, .. } => key
        }
    }

    /// Extracts the key of the record for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> record::Key {
        match self {
            PutRecordError::QuorumFailed { key, .. } => key,
            PutRecordError::Timeout { key, .. } => key,
            PutRecordError::LocalStorageError { key, .. } => key,
        }
    }
}

/// The result of [`Kademlia::bootstrap`].
pub type BootstrapResult = Result<BootstrapOk, BootstrapError>;

/// The successful result of [`Kademlia::bootstrap`].
#[derive(Debug, Clone)]
pub struct BootstrapOk {
    pub peer: PeerId
}

/// The error result of [`Kademlia::bootstrap`].
#[derive(Debug, Clone)]
pub enum BootstrapError {
    Timeout { peer: PeerId }
}

/// The result of [`Kademlia::get_closest_peers`].
pub type GetClosestPeersResult = Result<GetClosestPeersOk, GetClosestPeersError>;

/// The successful result of [`Kademlia::get_closest_peers`].
#[derive(Debug, Clone)]
pub struct GetClosestPeersOk {
    pub key: Vec<u8>,
    pub peers: Vec<PeerId>
}

/// The error result of [`Kademlia::get_closest_peers`].
#[derive(Debug, Clone)]
pub enum GetClosestPeersError {
    Timeout {
        key: Vec<u8>,
        peers: Vec<PeerId>
    }
}

impl GetClosestPeersError {
    /// Gets the key for which the operation failed.
    pub fn key(&self) -> &Vec<u8> {
        match self {
            GetClosestPeersError::Timeout { key, .. } => key,
        }
    }

    /// Extracts the key for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> Vec<u8> {
        match self {
            GetClosestPeersError::Timeout { key, .. } => key,
        }
    }
}

/// The result of [`Kademlia::get_providers`].
pub type GetProvidersResult = Result<GetProvidersOk, GetProvidersError>;

/// The successful result of [`Kademlia::get_providers`].
#[derive(Debug, Clone)]
pub struct GetProvidersOk {
    pub key: record::Key,
    pub providers: Vec<PeerId>,
    pub closest_peers: Vec<PeerId>
}

/// The error result of [`Kademlia::get_providers`].
#[derive(Debug, Clone)]
pub enum GetProvidersError {
    Timeout {
        key: record::Key,
        providers: Vec<PeerId>,
        closest_peers: Vec<PeerId>
    }
}

impl GetProvidersError {
    /// Gets the key for which the operation failed.
    pub fn key(&self) -> &record::Key {
        match self {
            GetProvidersError::Timeout { key, .. } => key,
        }
    }

    /// Extracts the key for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> record::Key {
        match self {
            GetProvidersError::Timeout { key, .. } => key,
        }
    }
}

/// The result of publishing a provider record.
pub type AddProviderResult = Result<AddProviderOk, AddProviderError>;

/// The successful result of publishing a provider record.
#[derive(Debug, Clone)]
pub struct AddProviderOk {
    pub key: record::Key,
}

/// The possible errors when publishing a provider record.
#[derive(Debug)]
pub enum AddProviderError {
    /// The query timed out.
    Timeout {
        key: record::Key,
    },
    /// The provider record could not be stored.
    LocalStorageError {
        key: record::Key,
        cause: store::Error
    }
}

impl AddProviderError {
    /// Gets the key for which the operation failed.
    pub fn key(&self) -> &record::Key {
        match self {
            AddProviderError::Timeout { key, .. } => key,
            AddProviderError::LocalStorageError { key, .. } => key,
        }
    }

    /// Extracts the key for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> record::Key {
        match self {
            AddProviderError::Timeout { key, .. } => key,
            AddProviderError::LocalStorageError { key, .. } => key,
        }
    }
}

impl From<kbucket::EntryView<kbucket::Key<PeerId>, Addresses>> for KadPeer {
    fn from(e: kbucket::EntryView<kbucket::Key<PeerId>, Addresses>) -> KadPeer {
        KadPeer {
            node_id: e.node.key.into_preimage(),
            multiaddrs: e.node.value.into_vec(),
            connection_ty: match e.status {
                NodeStatus::Connected => KadConnectionType::Connected,
                NodeStatus::Disconnected => KadConnectionType::NotConnected
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Internal query state

struct QueryInner {
    /// The query-specific state.
    info: QueryInfo,
    /// Addresses of peers discovered during a query.
    addresses: FnvHashMap<PeerId, SmallVec<[Multiaddr; 8]>>,
    /// A map of pending requests to peers.
    ///
    /// A request is pending if the targeted peer is not currently connected
    /// and these requests are sent as soon as a connection to the peer is established.
    pending_rpcs: SmallVec<[(PeerId, KademliaHandlerIn<QueryId>); K_VALUE.get()]>
}

impl QueryInner {
    fn new(info: QueryInfo) -> Self {
        QueryInner {
            info,
            addresses: Default::default(),
            pending_rpcs: SmallVec::default()
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AddProviderContext {
    Publish,
    Republish,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PutRecordContext {
    Publish,
    Republish,
    Replicate,
    Cache,
}

/// The internal query state.
#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryInfo {
    /// A bootstrapping query.
    Bootstrap {
        /// The targeted peer ID.
        peer: PeerId,
    },

    /// A query to find the closest peers to a key.
    GetClosestPeers { key: Vec<u8> },

    /// A query for the providers of a key.
    GetProviders {
        /// The key for which to search for providers.
        key: record::Key,
        /// The found providers.
        providers: Vec<PeerId>,
    },

    /// A query that searches for the closest closest nodes to a key to be
    /// used in a subsequent `AddProvider` query.
    PrepareAddProvider {
        key: record::Key,
        context: AddProviderContext,
    },

    /// A query that advertises the local node as a provider for a key.
    AddProvider {
        key: record::Key,
        provider_id: PeerId,
        external_addresses: Vec<Multiaddr>,
        context: AddProviderContext,
    },

    /// A query that searches for the closest closest nodes to a key to be used
    /// in a subsequent `PutValue` query.
    PreparePutRecord {
        record: Record,
        quorum: NonZeroUsize,
        context: PutRecordContext,
    },

    /// A query that replicates a record to other nodes.
    PutRecord {
        record: Record,
        quorum: NonZeroUsize,
        num_results: usize,
        context: PutRecordContext,
    },

    /// A query that searches for values for a key.
    GetRecord {
        /// The key to look for.
        key: record::Key,
        /// The records found.
        records: Vec<Record>,
        /// The number of records to look for.
        quorum: NonZeroUsize,
        /// The closest peer to `key` that did not return a record.
        ///
        /// When a record is found in a standard Kademlia query (quorum == 1),
        /// it is cached at this peer.
        cache_at: Option<kbucket::Key<PeerId>>,
    },
}

impl QueryInfo {
    /// Creates an event for a handler to issue an outgoing request in the
    /// context of a query.
    fn to_request(&self, query_id: QueryId) -> KademliaHandlerIn<QueryId> {
        match &self {
            QueryInfo::Bootstrap { peer } => KademliaHandlerIn::FindNodeReq {
                key: peer.clone().into_bytes(),
                user_data: query_id,
            },
            QueryInfo::GetClosestPeers { key, .. } => KademliaHandlerIn::FindNodeReq {
                key: key.clone(),
                user_data: query_id,
            },
            QueryInfo::GetProviders { key, .. } => KademliaHandlerIn::GetProvidersReq {
                key: key.clone(),
                user_data: query_id,
            },
            QueryInfo::PrepareAddProvider { key, .. } => KademliaHandlerIn::FindNodeReq {
                key: key.to_vec(),
                user_data: query_id,
            },
            QueryInfo::AddProvider {
                key,
                provider_id,
                external_addresses,
                ..
            } => KademliaHandlerIn::AddProvider {
                key: key.clone(),
                provider: crate::protocol::KadPeer {
                    node_id: provider_id.clone(),
                    multiaddrs: external_addresses.clone(),
                    connection_ty: crate::protocol::KadConnectionType::Connected,
                }
            },
            QueryInfo::GetRecord { key, .. } => KademliaHandlerIn::GetRecord {
                key: key.clone(),
                user_data: query_id,
            },
            QueryInfo::PreparePutRecord { record, .. } => KademliaHandlerIn::FindNodeReq {
                key: record.key.to_vec(),
                user_data: query_id,
            },
            QueryInfo::PutRecord { record, .. } => KademliaHandlerIn::PutRecord {
                record: record.clone(),
                user_data: query_id
            }
        }
    }
}

