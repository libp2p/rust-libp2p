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

use crate::{KBUCKET_PENDING_TIMEOUT, ADD_PROVIDER_INTERVAL};
use crate::addresses::Addresses;
use crate::handler::{KademliaHandler, KademliaHandlerEvent, KademliaHandlerIn};
use crate::kbucket::{self, KBucketsTable, NodeStatus};
use crate::protocol::{KadConnectionType, KadPeer};
use crate::query::{Query, QueryId, QueryPool, QueryConfig, QueryPoolState};
use crate::record::{MemoryRecordStorage, RecordStore, Record, RecordStorageError};
use fnv::{FnvHashMap, FnvHashSet};
use futures::{prelude::*, stream};
use libp2p_core::{ConnectedPoint, Multiaddr, PeerId};
use libp2p_swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters, ProtocolsHandler};
use multihash::Multihash;
use smallvec::SmallVec;
use std::{borrow::Cow, error, marker::PhantomData, time::Duration, num::NonZeroU8};
use std::collections::VecDeque;
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::{Instant, Interval};

mod test;

/// Network behaviour that handles Kademlia.
pub struct Kademlia<TSubstream, TStore: RecordStore = MemoryRecordStorage> {
    /// The Kademlia routing table.
    kbuckets: KBucketsTable<PeerId, Addresses>,

    /// An optional protocol name override to segregate DHTs in the network.
    protocol_name_override: Option<Cow<'static, [u8]>>,

    /// The currently active (i.e. in-progress) queries.
    queries: QueryPool<QueryInner>,

    /// List of peers known to be connected.
    connected_peers: FnvHashSet<PeerId>,

    /// A list of pending request to peers that are not currently connected.
    /// These requests are sent as soon as a connection to the peer is established.
    pending_rpcs: SmallVec<[(PeerId, KademliaHandlerIn<QueryId>); 8]>,

    /// List of values and peers that are providing them.
    ///
    /// Our local peer ID can be in this container.
    values_providers: FnvHashMap<Multihash, SmallVec<[PeerId; 20]>>,

    /// List of values that we are providing ourselves. Must be kept in sync with
    /// `values_providers`.
    providing_keys: FnvHashSet<Multihash>,

    /// List of providers to add to the topology as soon as we are in `poll()`.
    add_provider: SmallVec<[(Multihash, PeerId); 32]>,

    /// Interval to send `ADD_PROVIDER` messages to everyone.
    refresh_add_providers: stream::Fuse<Interval>,

    /// Queued events to return when the behaviour is being polled.
    queued_events: VecDeque<NetworkBehaviourAction<KademliaHandlerIn<QueryId>, KademliaEvent>>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,

    /// The record storage.
    records: TStore,
}

/// The configuration for the `Kademlia` behaviour.
///
/// The configuration is consumed by [`Kademlia::new`].
pub struct KademliaConfig<TStore = MemoryRecordStorage> {
    local_peer_id: PeerId,
    protocol_name_override: Option<Cow<'static, [u8]>>,
    query_config: QueryConfig,
    records: Option<TStore>
}

impl<TStore> KademliaConfig<TStore> {
    /// Creates a new KademliaConfig for the given local peer ID.
    pub fn new(local_peer_id: PeerId) -> Self {
        KademliaConfig {
            local_peer_id,
            protocol_name_override: None,
            query_config: QueryConfig::default(),
            records: None
        }
    }

    /// Sets a custom protocol name.
    ///
    /// Kademlia nodes only communicate with other nodes using the same protocol name. Using a
    /// custom name therefore allows to segregate the DHT from others, if that is desired.
    pub fn set_protocol_name(&mut self, name: impl Into<Cow<'static, [u8]>>) -> &mut Self {
        self.protocol_name_override = Some(name.into());
        self
    }

    /// Sets the record storage implementation to use.
    pub fn set_storage(&mut self, store: TStore) -> &mut Self {
        self.records = Some(store);
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
    /// The default replication factor is [`K_VALUE`].
    pub fn set_replication_factor(&mut self, replication_factor: usize) -> &mut Self {
        self.query_config.replication_factor = replication_factor;
        self
    }
}

impl<TSubstream, TStore> Kademlia<TSubstream, TStore>
where
    TStore: RecordStore
{
    /// Creates a new `Kademlia` network behaviour with the given configuration.
    pub fn new(config: KademliaConfig<TStore>) -> Self
    where
        TStore: Default
    {
        let local_key = kbucket::Key::new(config.local_peer_id);
        let pending_rpcs = SmallVec::with_capacity(config.query_config.replication_factor);
        Kademlia {
            kbuckets: KBucketsTable::new(local_key, KBUCKET_PENDING_TIMEOUT),
            protocol_name_override: config.protocol_name_override,
            queued_events: VecDeque::with_capacity(config.query_config.replication_factor),
            queries: QueryPool::new(config.query_config),
            connected_peers: Default::default(),
            pending_rpcs,
            values_providers: FnvHashMap::default(),
            providing_keys: FnvHashSet::default(),
            refresh_add_providers: Interval::new_interval(ADD_PROVIDER_INTERVAL).fuse(),
            add_provider: SmallVec::new(),
            records: config.records.unwrap_or_default(),
            marker: PhantomData,
        }
    }

    /// Adds a known address of a peer participating in the Kademlia DHT to the
    /// routing table.
    ///
    /// This allows prepopulating the Kademlia routing table with known addresses,
    /// e.g. for bootstrap nodes.
    pub fn add_address(&mut self, peer_id: &PeerId, address: Multiaddr) {
        let key = kbucket::Key::new(peer_id.clone());
        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(mut entry, _) => {
                entry.value().insert(address);
            }
            kbucket::Entry::Pending(mut entry, _) => {
                entry.value().insert(address);
            }
            kbucket::Entry::Absent(entry) => {
                let mut addresses = Addresses::new();
                addresses.insert(address);
                match entry.insert(addresses, NodeStatus::Disconnected) {
                    kbucket::InsertResult::Inserted => {
                        let event = KademliaEvent::RoutingUpdated {
                            new_peer: peer_id.clone(),
                            old_peer: None,
                        };
                        self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    kbucket::InsertResult::Full => (),
                    kbucket::InsertResult::Pending { disconnected } => {
                        self.queued_events.push_back(NetworkBehaviourAction::DialPeer {
                            peer_id: disconnected.into_preimage(),
                        })
                    },
                }
                return;
            },
            kbucket::Entry::SelfEntry => return,
        };
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
        K: Into<Multihash> + Clone
    {
        let multihash = key.into();
        let info = QueryInfo::GetClosestPeers { key: multihash.clone() };
        let target = kbucket::Key::new(multihash);
        let peers = self.kbuckets.closest_keys(&target);
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Performs a lookup for a record in the DHT.
    ///
    /// The result of this operation is delivered in [`KademliaEvent::GetRecordResult`].
    pub fn get_record(&mut self, key: &Multihash, quorum: Quorum) {
        let quorum = quorum.eval(self.queries.config().replication_factor);
        let mut records = Vec::with_capacity(quorum);

        if let Some(record) = self.records.get(key) {
            records.push(record.into_owned());
            if quorum == 1 {
                self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                    KademliaEvent::GetRecordResult(Ok(GetRecordOk { records }))
                ));
                return;
            }
        }

        let target = kbucket::Key::from(key.clone());
        let info = QueryInfo::GetRecord { key: key.clone(), records, quorum };
        let peers = self.kbuckets.closest_keys(&target);
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Stores a record in the DHT.
    ///
    /// The result of this operation is delivered in [`KademliaEvent::PutRecordResult`].
    ///
    /// The record is always stored locally.
    pub fn put_record(&mut self, record: Record, quorum: Quorum) {
        let quorum = quorum.eval(self.queries.config().replication_factor);
        if let Err(error) = self.records.put(record.clone()) {
            self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                KademliaEvent::PutRecordResult(Err(
                    PutRecordError::LocalStorageError {
                        key: record.key,
                        cause: error,
                    }
                ))
            ));
        } else {
            let target = kbucket::Key::from(record.key.clone());
            let peers = self.kbuckets.closest_keys(&target);
            let info = QueryInfo::PreparePutRecord {
                key: record.key,
                value: record.value,
                quorum
            };
            let inner = QueryInner::new(info);
            self.queries.add_iter_closest(target.clone(), peers, inner);
        }
    }

    /// Get a mutable reference to internal record store.
    pub fn store_mut(&mut self) -> &mut TStore {
        &mut self.records
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
    /// The results of this operation are delivered in [`KademliaEvent::BootstrapResult`],
    /// with one event per query.
    ///
    /// > **Note**: Bootstrapping requires at least one node of the DHT to be known.
    /// > See [`Kademlia::add_address`].
    pub fn bootstrap(&mut self) {
        let local_key = self.kbuckets.local_key().clone();
        let info = QueryInfo::Bootstrap { target: local_key.preimage().clone() };
        let peers = self.kbuckets.closest_keys(&local_key).collect::<Vec<_>>();
        // TODO: Emit error if `peers` is empty? BootstrapError::NoPeers?
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(local_key, peers, inner);
    }

    /// Registers the local node as the provider of a value for the given key.
    ///
    /// This operation will start periodically sending `ADD_PROVIDER` messages to the nodes
    /// closest to the key, so that other nodes can find this node as a result of
    /// a `GET_PROVIDERS` iterative request on the DHT.
    ///
    /// In contrast to the standard Kademlia push-based model for content distribution
    /// implemented by [`Kademlia::put_record`], the provider API implements a
    /// pull-based model that may be used in addition, or as an alternative to,
    /// the push-based model. The means by which the actual value is obtained
    /// from a provider is out of scope of the libp2p Kademlia provider API.
    ///
    /// The periodic results of the provider announcements sent by this node are delivered
    /// in [`KademliaEvent::AddProviderResult`].
    pub fn start_providing(&mut self, key: Multihash) {
        self.providing_keys.insert(key.clone());
        let providers = self.values_providers.entry(key).or_insert_with(Default::default);
        let local_id = self.kbuckets.local_key().preimage();
        if !providers.iter().any(|peer_id| peer_id == local_id) {
            providers.push(local_id.clone());
        }

        // Trigger the next refresh now.
        self.refresh_add_providers = Interval::new(Instant::now(), ADD_PROVIDER_INTERVAL).fuse();
    }

    /// Stops the local node from announcing that it is a provider for the given key.
    ///
    /// This is a local operation. The local node will still be considered as a
    /// provider for the key by other nodes until these provider records expire.
    pub fn stop_providing(&mut self, key: &Multihash) {
        self.providing_keys.remove(key);

        let providers = match self.values_providers.get_mut(key) {
            Some(p) => p,
            None => return,
        };

        if let Some(position) = providers.iter().position(|k| k == key) {
            providers.remove(position);
            providers.shrink_to_fit();
        }
    }

    /// Performs a lookup for providers of a value to the given key.
    ///
    /// The result of this operation is delivered in [`KademliaEvent::GetProvidersResult`].
    pub fn get_providers(&mut self, key: Multihash) {
        let info = QueryInfo::GetProviders {
            target: key.clone(),
            providers: Vec::new(),
        };
        let target = kbucket::Key::from(key);
        let peers = self.kbuckets.closest_keys(&target);
        let inner = QueryInner::new(info);
        self.queries.add_iter_closest(target.clone(), peers, inner);
    }

    /// Processes discovered peers from an iterative `Query`.
    fn discovered<'a, I>(&'a mut self, query_id: &QueryId, source: &PeerId, peers: I)
    where
        I: Iterator<Item=&'a KadPeer> + Clone
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
                query.inner.untrusted_addresses
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
                .take(self.queries.config().replication_factor)
                .map(KadPeer::from)
                .collect()
        }
    }

    /// Collects all peers who are known to be providers of the value for a given `Multihash`.
    fn provider_peers(&mut self, key: &Multihash, source: &PeerId) -> Vec<KadPeer> {
        let kbuckets = &mut self.kbuckets;
        self.values_providers
            .get(key)
            .into_iter()
            .flat_map(|peers| peers)
            .filter_map(move |p|
                if p != source {
                    let key = kbucket::Key::new(p.clone());
                    kbuckets.entry(&key).view().map(|e| KadPeer::from(e.to_owned()))
                } else {
                    None
                })
            .collect()
    }

    /// Updates the connection status of a peer in the Kademlia routing table.
    fn connection_updated(&mut self, peer: PeerId, address: Option<Multiaddr>, new_status: NodeStatus) {
        let key = kbucket::Key::new(peer.clone());
        match self.kbuckets.entry(&key) {
            kbucket::Entry::Present(mut entry, old_status) => {
                if let Some(address) = address {
                    entry.value().insert(address);
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

            kbucket::Entry::Absent(entry) => if new_status == NodeStatus::Connected {
                let mut addresses = Addresses::new();
                if let Some(address) = address {
                    addresses.insert(address);
                }
                match entry.insert(addresses, new_status) {
                    kbucket::InsertResult::Inserted => {
                        let event = KademliaEvent::RoutingUpdated {
                            new_peer: peer.clone(),
                            old_peer: None,
                        };
                        self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    kbucket::InsertResult::Full => (),
                    kbucket::InsertResult::Pending { disconnected } => {
                        debug_assert!(!self.connected_peers.contains(disconnected.preimage()));
                        self.queued_events.push_back(NetworkBehaviourAction::DialPeer {
                            peer_id: disconnected.into_preimage(),
                        })
                    },
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
            QueryInfo::Bootstrap { target } => {
                let local_key = self.kbuckets.local_key().clone();
                if &target == local_key.preimage() {
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
                        let info = QueryInfo::Bootstrap { target: target.clone().into_preimage() };
                        let peers = self.kbuckets.closest_keys(&target);
                        let inner = QueryInner::new(info);
                        self.queries.add_iter_closest(target.clone(), peers, inner);
                    }
                }
                Some(KademliaEvent::BootstrapResult(Ok(BootstrapOk { peer: target })))
            }

            QueryInfo::GetClosestPeers { key, .. } => {
                Some(KademliaEvent::GetClosestPeersResult(Ok(
                    GetClosestPeersOk { key, peers: result.peers.collect() }
                )))
            }

            QueryInfo::GetProviders { target, providers } => {
                Some(KademliaEvent::GetProvidersResult(Ok(
                    GetProvidersOk {
                        key: target,
                        providers,
                        closest_peers: result.peers.collect()
                    }
                )))
            }

            QueryInfo::PrepareAddProvider { target } => {
                let closest_peers = result.peers.map(kbucket::Key::from);
                let provider_id = params.local_peer_id().clone();
                let external_addresses = params.external_addresses().collect();
                let inner = QueryInner::new(QueryInfo::AddProvider {
                    target,
                    provider_id,
                    external_addresses
                });
                self.queries.add_fixed(closest_peers, inner);
                None
            }

            QueryInfo::AddProvider { target, .. } => {
                Some(KademliaEvent::AddProviderResult(Ok(
                    AddProviderOk { key: target }
                )))
            }

            QueryInfo::GetRecord { key, records, quorum, .. } => {
                let result = if records.len() >= quorum {
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

            QueryInfo::PreparePutRecord { key, value, quorum } => {
                let closest_peers = result.peers.map(kbucket::Key::from);
                let info = QueryInfo::PutRecord { key, value, num_results: 0, quorum };
                let inner = QueryInner::new(info);
                self.queries.add_fixed(closest_peers, inner);
                None
            }

            QueryInfo::PutRecord { key, num_results, quorum, .. } => {
                let result = if num_results >= quorum {
                    Ok(PutRecordOk { key })
                } else {
                    Err(PutRecordError::QuorumFailed { key, num_results, quorum })
                };
                Some(KademliaEvent::PutRecordResult(result))
            }
        }
    }

    /// Handles a query that timed out.
    fn query_timeout(&self, query: Query<QueryInner>) -> KademliaEvent {
        let result = query.into_result();
        match result.inner.info {
            QueryInfo::Bootstrap { target } =>
                KademliaEvent::BootstrapResult(Err(
                    BootstrapError::Timeout { peer: target })),

            QueryInfo::PrepareAddProvider { target } =>
                KademliaEvent::AddProviderResult(Err(
                    AddProviderError::Timeout { key: target })),

            QueryInfo::AddProvider { target, .. } =>
                KademliaEvent::AddProviderResult(Err(
                    AddProviderError::Timeout { key: target })),

            QueryInfo::GetClosestPeers { key } =>
                KademliaEvent::GetClosestPeersResult(Err(
                    GetClosestPeersError::Timeout {
                        key,
                        peers: result.peers.collect()
                    })),

            QueryInfo::PreparePutRecord { key, quorum, .. } =>
                KademliaEvent::PutRecordResult(Err(
                    PutRecordError::Timeout { key, num_results: 0, quorum })),

            QueryInfo::PutRecord { key, num_results, quorum, .. } =>
                KademliaEvent::PutRecordResult(Err(
                    PutRecordError::Timeout { key, num_results, quorum })),

            QueryInfo::GetRecord { key, records, quorum } =>
                KademliaEvent::GetRecordResult(Err(
                    GetRecordError::Timeout { key, records, quorum })),

            QueryInfo::GetProviders { target, providers } =>
                KademliaEvent::GetProvidersResult(Err(
                    GetProvidersError::Timeout {
                        key: target,
                        providers,
                        closest_peers: result.peers.collect()
                    })),
        }
    }
}

impl<TSubstream, TStore> NetworkBehaviour for Kademlia<TSubstream, TStore>
where
    TSubstream: AsyncRead + AsyncWrite,
    TStore: RecordStore,
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
        let mut out_list =
            if let kbucket::Entry::Present(mut entry, _) = self.kbuckets.entry(&key) {
                entry.value().iter().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            };

        // We add to that a temporary list of addresses from the ongoing queries.
        for query in self.queries.iter() {
            if let Some(addrs) = query.inner.untrusted_addresses.get(peer_id) {
                for addr in addrs {
                    out_list.push(addr.clone());
                }
            }
        }

        out_list
    }

    fn inject_connected(&mut self, id: PeerId, endpoint: ConnectedPoint) {
        while let Some(pos) = self.pending_rpcs.iter().position(|(p, _)| p == &id) {
            let (_, rpc) = self.pending_rpcs.remove(pos);
            self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: rpc,
            });
        }

        let address = match endpoint {
            ConnectedPoint::Dialer { address } => Some(address),
            ConnectedPoint::Listener { .. } => None,
        };

        self.connection_updated(id.clone(), address, NodeStatus::Connected);
        self.connected_peers.insert(id);
    }

    fn inject_addr_reach_failure(&mut self, peer_id: Option<&PeerId>, addr: &Multiaddr, _: &dyn error::Error) {
        if let Some(peer_id) = peer_id {
            let key = kbucket::Key::new(peer_id.clone());

            if let Some(addrs) = self.kbuckets.entry(&key).value() {
                // TODO: don't remove the address if the error is that we are already connected
                //       to this peer
                addrs.remove(addr);
            }

            for query in self.queries.iter_mut() {
                if let Some(addrs) = query.inner.untrusted_addresses.get_mut(&peer_id) {
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
                // TODO: Remove the old address, i.e. from `_old`?
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
                let closer_peers = self.find_closest(&kbucket::Key::from(key), &source);
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

            KademliaHandlerEvent::AddProvider { key, provider_peer } => {
                self.queued_events.push_back(NetworkBehaviourAction::GenerateEvent(
                    KademliaEvent::Discovered {
                        peer_id: provider_peer.node_id.clone(),
                        addresses: provider_peer.multiaddrs.clone(),
                        ty: provider_peer.connection_ty,
                    }));
                // TODO: Expire provider records.
                self.add_provider.push((key, provider_peer.node_id));
            }

            KademliaHandlerEvent::GetValue { key, request_id } => {
                let (result, closer_peers) = match self.records.get(&key) {
                    Some(record) => {
                        (Some(record.into_owned()), Vec::new())
                    },
                    None => {
                        let closer_peers = self.find_closest(&kbucket::Key::from(key), &source);
                        (None, closer_peers)
                    }
                };

                self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: source,
                    event: KademliaHandlerIn::GetValueRes {
                        result,
                        closer_peers,
                        request_id,
                    },
                });
            }

            KademliaHandlerEvent::GetValueRes {
                result,
                closer_peers,
                user_data,
            } => {
                if let Some(query) = self.queries.get_mut(&user_data) {
                    if let QueryInfo::GetRecord { records, quorum, .. } = &mut query.inner.info {
                        if let Some(result) = result {
                            records.push(result);
                            if records.len() == *quorum {
                                query.finish()
                            }
                        }
                    }
                }

                self.discovered(&user_data, &source, closer_peers.iter());
            }

            KademliaHandlerEvent::PutValue {
                key,
                value,
                request_id
            } => {
                // TODO: Log errors and immediately reset the stream on error instead of letting the request time out.
                if let Ok(()) = self.records.put(Record { key: key.clone(), value: value.clone() }) {
                    self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                        peer_id: source,
                        event: KademliaHandlerIn::PutValueRes {
                            key,
                            value,
                            request_id,
                        },
                    });
                }
            }

            KademliaHandlerEvent::PutValueRes {
                key: _,
                user_data,
            } => {
                if let Some(query) = self.queries.get_mut(&user_data) {
                    query.on_success(&source, vec![]);
                    if let QueryInfo::PutRecord {
                        num_results, quorum, ..
                    } = &mut query.inner.info {
                        *num_results += 1;
                        if *num_results == *quorum {
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

        // Flush the changes to the topology that we want to make.
        for (key, provider) in self.add_provider.drain() {
            // Don't add ourselves to the providers.
            if &provider == self.kbuckets.local_key().preimage() {
                continue;
            }
            let providers = self.values_providers.entry(key).or_insert_with(Default::default);
            if !providers.iter().any(|peer_id| peer_id == &provider) {
                providers.push(provider);
            }
        }
        self.add_provider.shrink_to_fit();

        // Handle `refresh_add_providers`.
        match self.refresh_add_providers.poll() {
            Ok(Async::NotReady) => {},
            Ok(Async::Ready(Some(_))) => {
                for target in self.providing_keys.clone().into_iter() {
                    let info = QueryInfo::PrepareAddProvider { target: target.clone() };
                    let target = kbucket::Key::from(target);
                    let peers = self.kbuckets.closest_keys(&target);
                    let inner = QueryInner::new(info);
                    self.queries.add_iter_closest(target.clone(), peers, inner);
                }
            },
            // Ignore errors.
            Ok(Async::Ready(None)) | Err(_) => {},
        }

        loop {
            // Drain queued events first.
            if let Some(event) = self.queued_events.pop_front() {
                return Async::Ready(event);
            }

            // Drain applied pending entries from the routing table.
            if let Some(entry) = self.kbuckets.take_applied_pending() {
                let event = KademliaEvent::RoutingUpdated {
                    new_peer: entry.inserted.into_preimage(),
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
                        let event = self.query_timeout(q);
                        return Async::Ready(NetworkBehaviourAction::GenerateEvent(event))
                    }
                    QueryPoolState::Waiting(Some((query, peer_id))) => {
                        let event = query.inner.info.to_request(query.id());
                        if self.connected_peers.contains(&peer_id) {
                            self.queued_events.push_back(NetworkBehaviourAction::SendEvent {
                                peer_id, event
                            });
                        } else if &peer_id != self.kbuckets.local_key().preimage() {
                            self.pending_rpcs.push((peer_id.clone(), event));
                            self.queued_events.push_back(NetworkBehaviourAction::DialPeer {
                                peer_id
                            });
                        }
                    }
                    QueryPoolState::Waiting(None) | QueryPoolState::Idle => break,
                }
            }

            // No finished query or finished write produced an immediate event.
            // If no new events have been queued either, signal `NotReady` to
            // be polled again later.
            if self.queued_events.is_empty() {
                return Async::NotReady
            }
        }
    }
}

/// A quorum w.r.t. the configured replication factor specifies the minimum number of distinct
/// nodes that must be successfully contacted in order for a query to succeed.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Quorum {
    One,
    Majority,
    All,
    N(NonZeroU8)
}

impl Quorum {
    /// Evaluate the quorum w.r.t a given total (number of peers).
    fn eval(&self, total: usize) -> usize {
        match self {
            Quorum::One => 1,
            Quorum::Majority => total / 2 + 1,
            Quorum::All => total,
            Quorum::N(n) => usize::min(total, n.get() as usize)
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Events

/// The events produced by the `Kademlia` behaviour.
///
/// See [`Kademlia::poll`].
#[derive(Debug, Clone)]
pub enum KademliaEvent {
    /// The result of [`Kademlia::bootstrap`].
    BootstrapResult(BootstrapResult),

    /// The result of [`Kademlia::get_closest_peers`].
    GetClosestPeersResult(GetClosestPeersResult),

    /// The result of [`Kademlia::get_providers`].
    GetProvidersResult(GetProvidersResult),

    /// The result of periodic queries initiated by [`Kademlia::start_providing`].
    AddProviderResult(AddProviderResult),

    /// The result of [`Kademlia::get_record`].
    GetRecordResult(GetRecordResult),

    /// The result of [`Kademlia::put_record`].
    PutRecordResult(PutRecordResult),

    /// A new peer in the DHT has been discovered during an iterative query.
    Discovered {
        /// The identifier of the discovered peer.
        peer_id: PeerId,
        /// The known addresses of the discovered peer.
        addresses: Vec<Multiaddr>,
        /// The connection status of the reported peer, as seen by the local
        /// peer.
        ty: KadConnectionType,
    },

    /// A peer in the DHT has been added to the routing table.
    RoutingUpdated {
        /// The ID of the peer that was added to a bucket in the routing table.
        new_peer: PeerId,
        /// The ID of the peer that was evicted from the routing table to make
        /// room for the new peer, if any.
        old_peer: Option<PeerId>,
    },
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
    NotFound { key: Multihash, closest_peers: Vec<PeerId> },
    QuorumFailed { key: Multihash, records: Vec<Record>, quorum: usize },
    Timeout { key: Multihash, records: Vec<Record>, quorum: usize }
}

impl GetRecordError {
    /// Gets the key of the record for which the operation failed.
    pub fn key(&self) -> &Multihash {
        match self {
            GetRecordError::QuorumFailed { key, .. } => key,
            GetRecordError::Timeout { key, .. } => key,
            GetRecordError::NotFound { key, .. } => key,
        }
    }

    /// Extracts the key of the record for which the operation failed,
    /// consuming the error.
    fn into_key(self) -> Multihash {
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
    pub key: Multihash
}

/// The error result of [`Kademlia::put_record`].
#[derive(Debug, Clone)]
pub enum PutRecordError {
    QuorumFailed { key: Multihash, num_results: usize, quorum: usize },
    Timeout { key: Multihash, num_results: usize, quorum: usize },
    LocalStorageError { key: Multihash, cause: RecordStorageError },
}

impl PutRecordError {
    /// Gets the key of the record for which the operation failed.
    pub fn key(&self) -> &Multihash {
        match self {
            PutRecordError::QuorumFailed { key, .. } => key,
            PutRecordError::Timeout { key, .. } => key,
            PutRecordError::LocalStorageError { key, .. } => key
        }
    }

    /// Extracts the key of the record for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> Multihash {
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
    pub key: Multihash,
    pub peers: Vec<PeerId>
}

/// The error result of [`Kademlia::get_closest_peers`].
#[derive(Debug, Clone)]
pub enum GetClosestPeersError {
    Timeout { key: Multihash, peers: Vec<PeerId> }
}

impl GetClosestPeersError {
    /// Gets the key for which the operation failed.
    pub fn key(&self) -> &Multihash {
        match self {
            GetClosestPeersError::Timeout { key, .. } => key,
        }
    }

    /// Extracts the key for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> Multihash {
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
    pub key: Multihash,
    pub providers: Vec<PeerId>,
    pub closest_peers: Vec<PeerId>
}

/// The error result of [`Kademlia::get_providers`].
#[derive(Debug, Clone)]
pub enum GetProvidersError {
    Timeout {
        key: Multihash,
        providers: Vec<PeerId>,
        closest_peers: Vec<PeerId>
    }
}

impl GetProvidersError {
    /// Gets the key for which the operation failed.
    pub fn key(&self) -> &Multihash {
        match self {
            GetProvidersError::Timeout { key, .. } => key,
        }
    }

    /// Extracts the key for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> Multihash {
        match self {
            GetProvidersError::Timeout { key, .. } => key,
        }
    }
}

/// The result of periodic queries initiated by [`Kademlia::start_providing`].
pub type AddProviderResult = Result<AddProviderOk, AddProviderError>;

/// The successful result of a periodic query initiated by [`Kademlia::start_providing`].
#[derive(Debug, Clone)]
pub struct AddProviderOk {
    pub key: Multihash,
}

/// The error result of a periodic query initiated by [`Kademlia::start_providing`].
#[derive(Debug, Clone)]
pub enum AddProviderError {
    Timeout {
        key: Multihash,
    }
}

impl AddProviderError {
    /// Gets the key for which the operation failed.
    pub fn key(&self) -> &Multihash {
        match self {
            AddProviderError::Timeout { key, .. } => key,
        }
    }

    /// Extracts the key for which the operation failed,
    /// consuming the error.
    pub fn into_key(self) -> Multihash {
        match self {
            AddProviderError::Timeout { key, .. } => key,
        }
    }
}

impl From<kbucket::EntryView<PeerId, Addresses>> for KadPeer {
    fn from(e: kbucket::EntryView<PeerId, Addresses>) -> KadPeer {
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
    info: QueryInfo,
    untrusted_addresses: FnvHashMap<PeerId, SmallVec<[Multiaddr; 8]>>,
}

impl QueryInner {
    fn new(info: QueryInfo) -> Self {
        QueryInner {
            info,
            untrusted_addresses: Default::default()
        }
    }
}

/// The internal query state.
#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryInfo {
    /// A bootstrapping query.
    Bootstrap {
        /// The targeted peer ID.
        target: PeerId,
    },

    /// A query to find the closest peers to a key.
    GetClosestPeers { key: Multihash },

    /// A query for the providers of a key.
    GetProviders {
        /// Target we are searching the providers of.
        target: Multihash,
        /// Results to return. Filled over time.
        providers: Vec<PeerId>,
    },

    /// A query that searches for the closest closest nodes to a key to be
    /// used in a subsequent `AddProvider` query.
    PrepareAddProvider {
        target: Multihash
    },

    /// A query that advertises the local node as a provider for a key.
    AddProvider {
        target: Multihash,
        provider_id: PeerId,
        external_addresses: Vec<Multiaddr>,
    },

    /// A query that searches for the closest closest nodes to a key to be used
    /// in a subsequent `PutValue` query.
    PreparePutRecord {
        key: Multihash,
        value: Vec<u8>,
        quorum: usize,
    },

    /// A query that replicates a record to other nodes.
    PutRecord {
        key: Multihash,
        value: Vec<u8>,
        num_results: usize,
        quorum: usize,
    },

    /// A query that searches for values for a key.
    GetRecord {
        /// The key to look for.
        key: Multihash,
        /// The records found.
        records: Vec<Record>,
        /// The number of records to look for.
        quorum: usize,
    },
}

impl QueryInfo {
    /// Creates an event for a handler to issue an outgoing request in the
    /// context of a query.
    fn to_request(&self, query_id: QueryId) -> KademliaHandlerIn<QueryId> {
        match &self {
            QueryInfo::Bootstrap { target } => KademliaHandlerIn::FindNodeReq {
                key: target.clone().into(),
                user_data: query_id,
            },
            QueryInfo::GetClosestPeers { key, .. } => KademliaHandlerIn::FindNodeReq {
                key: key.clone(),
                user_data: query_id,
            },
            QueryInfo::GetProviders { target, .. } => KademliaHandlerIn::GetProvidersReq {
                key: target.clone(),
                user_data: query_id,
            },
            QueryInfo::PrepareAddProvider { target } => KademliaHandlerIn::FindNodeReq {
                key: target.clone(),
                user_data: query_id,
            },
            QueryInfo::AddProvider {
                target,
                provider_id,
                external_addresses
            } => KademliaHandlerIn::AddProvider {
                key: target.clone(),
                provider_peer: crate::protocol::KadPeer {
                    node_id: provider_id.clone(),
                    multiaddrs: external_addresses.clone(),
                    connection_ty: crate::protocol::KadConnectionType::Connected,
                }
            },
            QueryInfo::GetRecord { key, .. } => KademliaHandlerIn::GetValue {
                key: key.clone(),
                user_data: query_id,
            },
            QueryInfo::PreparePutRecord { key, .. } => KademliaHandlerIn::FindNodeReq {
                key: key.clone(),
                user_data: query_id,
            },
            QueryInfo::PutRecord { key, value, .. } => KademliaHandlerIn::PutValue {
                key: key.clone(),
                value: value.clone(),
                user_data: query_id
            }
        }
    }
}

