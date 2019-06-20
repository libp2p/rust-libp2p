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

use crate::addresses::Addresses;
use crate::handler::{KademliaHandler, KademliaHandlerEvent, KademliaHandlerIn};
use crate::kbucket::{self, KBucketsTable, NodeStatus};
use crate::protocol::{KadConnectionType, KadPeer};
use crate::query::{QueryConfig, Query, QueryState};
use crate::write::WriteState;
use crate::record::{MemoryRecordStorage, RecordStore, Record, RecordStorageError};
use fnv::{FnvHashMap, FnvHashSet};
use futures::{prelude::*, stream};
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, Multiaddr, PeerId};
use multihash::Multihash;
use smallvec::SmallVec;
use std::{borrow::Cow, error, iter::FromIterator, marker::PhantomData, num::NonZeroU8, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::{Instant, Interval};

mod test;

/// Network behaviour that handles Kademlia.
pub struct Kademlia<TSubstream, TRecordStorage: RecordStore = MemoryRecordStorage> {
    /// Storage for the nodes. Contains the known multiaddresses for this node.
    kbuckets: KBucketsTable<PeerId, Addresses>,

    /// If `Some`, we overwrite the Kademlia protocol name with this one.
    protocol_name_override: Option<Cow<'static, [u8]>>,

    /// All the iterative queries we are currently performing, with their ID. The last parameter
    /// is the list of accumulated providers for `GET_PROVIDERS` queries.
    active_queries: FnvHashMap<QueryId, Query<QueryInfo, PeerId>>,

    /// All the `PUT_VALUE` actions we are currently performing
    active_writes: FnvHashMap<QueryId, WriteState<PeerId, Multihash>>,

    /// List of peers the swarm is connected to.
    connected_peers: FnvHashSet<PeerId>,

    /// Contains a list of peer IDs which we are not connected to, and an RPC query to send to them
    /// once they connect.
    pending_rpcs: SmallVec<[(PeerId, KademliaHandlerIn<QueryId>); 8]>,

    /// Identifier for the next query that we start.
    next_query_id: QueryId,

    /// List of values and peers that are providing them.
    ///
    /// Our local peer ID can be in this container.
    values_providers: FnvHashMap<Multihash, SmallVec<[PeerId; 20]>>,

    /// List of values that we are providing ourselves. Must be kept in sync with
    /// `values_providers`.
    providing_keys: FnvHashSet<Multihash>,

    /// Interval to send `ADD_PROVIDER` messages to everyone.
    refresh_add_providers: stream::Fuse<Interval>,

    /// The configuration for iterative queries.
    query_config: QueryConfig,

    /// Queued events to return when the behaviour is being polled.
    queued_events: SmallVec<[NetworkBehaviourAction<KademliaHandlerIn<QueryId>, KademliaOut>; 32]>,

    /// List of providers to add to the topology as soon as we are in `poll()`.
    add_provider: SmallVec<[(Multihash, PeerId); 32]>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,

    /// The records that we keep.
    records: TRecordStorage,
}

/// Opaque type. Each query that we start gets a unique number.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct QueryId(usize);

/// Information about a query.
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueryInfo {
    /// What we are querying and why.
    inner: QueryInfoInner,
    /// Temporary addresses used when trying to reach nodes.
    untrusted_addresses: FnvHashMap<PeerId, SmallVec<[Multiaddr; 8]>>,
}

/// Additional information about the query.
#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryInfoInner {
    /// The query was created for the Kademlia initialization process.
    Initialization {
        /// Hash we're targetting to insert ourselves in the k-buckets.
        target: PeerId,
    },

    /// The user requested a `FIND_PEER` query to be performed. It should be reported when finished.
    FindPeer(PeerId),

    /// The user requested a `GET_PROVIDERS` query to be performed. It should be reported when
    /// finished.
    GetProviders {
        /// Target we are searching the providers of.
        target: Multihash,
        /// Results to return. Filled over time.
        pending_results: Vec<PeerId>,
    },

    /// We are traversing towards `target` and should add an `ADD_PROVIDER` message to the peers
    /// of the outcome with our own identity.
    AddProvider {
        /// Which hash we're targetting.
        target: Multihash,
    },

    /// Put the value to the dht records
    PutValue {
        /// The key of the record being inserted
        key: Multihash,
        /// The value of the record being inserted
        value: Vec<u8>,
    },

    /// Get value from the dht record
    GetValue {
        /// The key we're looking for
        key: Multihash,
        /// The results from peers are stored here
        results: Vec<Record>,
        /// The number of results to look for.
        num_results: usize,
    },
}

impl Into<kbucket::Key<QueryInfo>> for QueryInfo {
    fn into(self) -> kbucket::Key<QueryInfo> {
        kbucket::Key::new(self)
    }
}

impl AsRef<[u8]> for QueryInfo {
    fn as_ref(&self) -> &[u8] {
        match &self.inner {
            QueryInfoInner::Initialization { target } => target.as_ref(),
            QueryInfoInner::FindPeer(peer) => peer.as_ref(),
            QueryInfoInner::GetProviders { target, .. } => target.as_bytes(),
            QueryInfoInner::AddProvider { target } => target.as_bytes(),
            QueryInfoInner::GetValue { key, .. } => key.as_bytes(),
            QueryInfoInner::PutValue { key, .. } => key.as_bytes(),
        }
    }
}

impl QueryInfo {
    /// Creates the corresponding RPC request to send to remote.
    fn to_rpc_request<TUserData>(&self, user_data: TUserData) -> KademliaHandlerIn<TUserData> {
        match &self.inner {
            QueryInfoInner::Initialization { target } => KademliaHandlerIn::FindNodeReq {
                key: target.clone().into(),
                user_data,
            },
            QueryInfoInner::FindPeer(key) => KademliaHandlerIn::FindNodeReq {
                // TODO: Change the `key` of `QueryInfoInner::FindPeer` to be a `Multihash`.
                key: key.clone().into(),
                user_data,
            },
            QueryInfoInner::GetProviders { target, .. } => KademliaHandlerIn::GetProvidersReq {
                key: target.clone(),
                user_data,
            },
            QueryInfoInner::AddProvider { .. } => KademliaHandlerIn::FindNodeReq {
                key: unimplemented!(), // TODO: target.clone(),
                user_data,
            },
            QueryInfoInner::GetValue { key, .. } => KademliaHandlerIn::GetValue {
                key: key.clone(),
                user_data,
            },
            QueryInfoInner::PutValue { key, .. } => KademliaHandlerIn::FindNodeReq {
                key: key.clone(),
                user_data,
            }
        }
    }
}

impl<TSubstream, TRecordStorage> Kademlia<TSubstream, TRecordStorage>
where
    TRecordStorage: RecordStore
{
    /// Creates a new `Kademlia` network behaviour with the given local `PeerId`.
    pub fn new(local_peer_id: PeerId) -> Self
    where
        TRecordStorage: Default
    {
        Self::new_inner(local_peer_id, Default::default())
    }

    /// The same as `new`, but using a custom protocol name.
    ///
    /// Kademlia nodes only communicate with other nodes using the same protocol name. Using a
    /// custom name therefore allows to segregate the DHT from others, if that is desired.
    pub fn with_protocol_name(local_peer_id: PeerId, name: impl Into<Cow<'static, [u8]>>) -> Self
    where
        TRecordStorage: Default
    {
        let mut me = Kademlia::new_inner(local_peer_id, Default::default());
        me.protocol_name_override = Some(name.into());
        me
    }

    /// The same as `new`, but with a custom storage.
    ///
    /// The default records storage is in memory, this lets override the
    /// storage with user-defined behaviour
    pub fn with_storage(local_peer_id: PeerId, records: TRecordStorage) -> Self {
        Self::new_inner(local_peer_id, records)
    }

    /// Creates a `Kademlia`.
    ///
    /// Contrary to `new`, doesn't perform the initialization queries that store our local ID into
    /// the DHT and fill our buckets.
    #[deprecated(note="this function is now equivalent to new() and will be removed in the future")]
    pub fn without_init(local_peer_id: PeerId) -> Self
        where TRecordStorage: Default
    {
        Self::new_inner(local_peer_id, Default::default())
    }

    /// Adds a known address of a peer participating in the Kademlia DHT to the
    /// routing table.
    ///
    /// This allows prepopulating the Kademlia routing table with known
    /// addresses, so that they can be used immediately in following DHT
    /// operations involving one of these peers, without having to dial
    /// them upfront.
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
                        let event = KademliaOut::KBucketAdded {
                            peer_id: peer_id.clone(),
                            replaced: None,
                        };
                        self.queued_events.push(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    kbucket::InsertResult::Full => (),
                    kbucket::InsertResult::Pending { disconnected } => {
                        self.queued_events.push(NetworkBehaviourAction::DialPeer {
                            peer_id: disconnected.into_preimage(),
                        })
                    },
                }
                return;
            },
            kbucket::Entry::SelfEntry => return,
        };
    }

    /// Inner implementation of the constructors.
    fn new_inner(local_peer_id: PeerId, records: TRecordStorage) -> Self {
        let query_config = QueryConfig::default();

        Kademlia {
            kbuckets: KBucketsTable::new(kbucket::Key::new(local_peer_id), Duration::from_secs(60)),   // TODO: constant
            protocol_name_override: None,
            queued_events: SmallVec::new(),
            active_queries: Default::default(),
            active_writes: Default::default(),
            connected_peers: Default::default(),
            pending_rpcs: SmallVec::with_capacity(query_config.parallelism),
            next_query_id: QueryId(0),
            values_providers: FnvHashMap::default(),
            providing_keys: FnvHashSet::default(),
            refresh_add_providers: Interval::new_interval(Duration::from_secs(60)).fuse(),     // TODO: constant
            query_config,
            add_provider: SmallVec::new(),
            marker: PhantomData,
            records,
        }
    }

    /// Returns an iterator over all peer IDs of nodes currently contained in a bucket
    /// of the Kademlia routing table.
    pub fn kbuckets_entries(&mut self) -> impl Iterator<Item = &PeerId> {
        self.kbuckets.iter().map(|entry| entry.node.key.preimage())
    }

    /// Starts an iterative `FIND_NODE` request.
    ///
    /// This will eventually produce an event containing the nodes of the DHT closest to the
    /// requested `PeerId`.
    pub fn find_node(&mut self, peer_id: PeerId) {
        self.start_query(QueryInfoInner::FindPeer(peer_id));
    }

    /// Starts an iterative `GET_PROVIDERS` request.
    pub fn get_providers(&mut self, target: Multihash) {
        self.start_query(QueryInfoInner::GetProviders { target, pending_results: Vec::new() });
    }

    /// Starts an iterative `GET_VALUE` request.
    ///
    /// Returns a number of results that is in the interval [1, 20],
    /// if the user requested a larger amount of results it is cropped to 20.
    pub fn get_value(&mut self, key: &Multihash, num_results: NonZeroU8) {
        let num_results = usize::min(num_results.get() as usize, kbucket::MAX_NODES_PER_BUCKET);
        let mut results = Vec::with_capacity(num_results);

        if let Some(record) = self.records.get(key) {
            results.push(record.into_owned());
            if num_results == 1 {
                self.queued_events.push(NetworkBehaviourAction::GenerateEvent(
                    KademliaOut::GetValueResult(
                        GetValueResult::Found { results }
                )));
                return;
            }
        }

        self.start_query(QueryInfoInner::GetValue {
            key: key.clone(),
            results,
            num_results
        });
    }

    /// Starts an iterative `PUT_VALUE` request
    pub fn put_value(&mut self, key: Multihash, value: Vec<u8>) {
        if let Err(error) = self.records.put(Record { key: key.clone(), value: value.clone() }) {
            self.queued_events.push(NetworkBehaviourAction::GenerateEvent(
                KademliaOut::PutValueResult(
                    PutValueResult::Err { key, cause: error }
                )
            ));
        } else {
            self.start_query(QueryInfoInner::PutValue { key, value });
        }
    }

    /// Register the local node as the provider for the given key.
    ///
    /// This will periodically send `ADD_PROVIDER` messages to the nodes closest to the key. When
    /// someone performs a `GET_PROVIDERS` iterative request on the DHT, our local node will be
    /// returned as part of the results.
    ///
    /// The actual meaning of *providing* the value of a key is not defined, and is specific to
    /// the value whose key is the hash.
    pub fn add_providing(&mut self, key: Multihash) {
        self.providing_keys.insert(key.clone());
        let providers = self.values_providers.entry(key).or_insert_with(Default::default);
        let local_id = self.kbuckets.local_key().preimage();
        if !providers.iter().any(|peer_id| peer_id == local_id) {
            providers.push(local_id.clone());
        }

        // Trigger the next refresh now.
        self.refresh_add_providers = Interval::new(Instant::now(), Duration::from_secs(60)).fuse();
    }

    /// Cancels a registration done with `add_providing`.
    ///
    /// There doesn't exist any "remove provider" message to broadcast on the network, therefore we
    /// will still be registered as a provider in the DHT for as long as the timeout doesn't expire.
    pub fn remove_providing(&mut self, key: &Multihash) {
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

    /// Internal function that starts a query.
    fn start_query(&mut self, target: QueryInfoInner) {
        let query_id = self.next_query_id;
        self.next_query_id.0 += 1;

        let target = QueryInfo {
            inner: target,
            untrusted_addresses: Default::default(),
        };

        let target_key = kbucket::Key::new(target.clone());
        let known_closest_peers = self.kbuckets.closest_keys(&target_key);
        let query = Query::with_config(self.query_config.clone(), target, known_closest_peers);

        self.active_queries.insert(query_id, query);
    }

    /// Processes discovered peers from an iterative `Query`.
    fn discovered<'a, I>(&'a mut self, query_id: &QueryId, source: &PeerId, peers: I)
    where
        I: Iterator<Item=&'a KadPeer> + Clone
    {
        let local_id = self.kbuckets.local_key().preimage().clone();
        let others_iter = peers.filter(|p| p.node_id != local_id);

        for peer in others_iter.clone() {
            self.queued_events.push(NetworkBehaviourAction::GenerateEvent(
                KademliaOut::Discovered {
                    peer_id: peer.node_id.clone(),
                    addresses: peer.multiaddrs.clone(),
                    ty: peer.connection_ty,
                }
            ));
        }

        if let Some(query) = self.active_queries.get_mut(query_id) {
            for peer in others_iter.clone() {
                query.target_mut().untrusted_addresses
                    .insert(peer.node_id.clone(), peer.multiaddrs.iter().cloned().collect());
            }
            query.on_success(source, others_iter.cloned().map(|kp| kp.node_id))
        }
    }

    /// Finds the closest peers to a `target` in the context of a request by
    /// the `source` peer, such that the `source` peer is never included in the
    /// result.
    fn find_closest<T: Clone>(&mut self, target: &kbucket::Key<T>, source: &PeerId) -> Vec<KadPeer> {
        self.kbuckets
            .closest(target)
            .filter(|e| e.node.key.preimage() != source)
            .take(self.query_config.num_results)
            .map(KadPeer::from)
            .collect()
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

    /// Update the connection status of a peer in the Kademlia routing table.
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
                        let event = KademliaOut::KBucketAdded {
                            peer_id: peer.clone(),
                            replaced: None,
                        };
                        self.queued_events.push(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    kbucket::InsertResult::Full => (),
                    kbucket::InsertResult::Pending { disconnected } => {
                        debug_assert!(!self.connected_peers.contains(disconnected.preimage()));
                        self.queued_events.push(NetworkBehaviourAction::DialPeer {
                            peer_id: disconnected.into_preimage(),
                        })
                    },
                }
            },
            _ => {}
        }
    }
}

impl<TSubstream, TRecordStorage> NetworkBehaviour for Kademlia<TSubstream, TRecordStorage>
where
    TSubstream: AsyncRead + AsyncWrite,
    TRecordStorage: RecordStore,
{
    type ProtocolsHandler = KademliaHandler<TSubstream, QueryId>;
    type OutEvent = KademliaOut;

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
        for query in self.active_queries.values() {
            if let Some(addrs) = query.target().untrusted_addresses.get(peer_id) {
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
            self.queued_events.push(NetworkBehaviourAction::SendEvent {
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

            for query in self.active_queries.values_mut() {
                if let Some(addrs) = query.target_mut().untrusted_addresses.get_mut(&peer_id) {
                    addrs.retain(|a| a != addr);
                }
            }
        }
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        for query in self.active_queries.values_mut() {
            query.on_failure(peer_id);
        }
        for write in self.active_writes.values_mut() {
            write.inject_write_error(peer_id);
        }
    }

    fn inject_disconnected(&mut self, id: &PeerId, _old_endpoint: ConnectedPoint) {
        for query in self.active_queries.values_mut() {
            query.on_failure(id);
        }
        for write in self.active_writes.values_mut() {
            write.inject_write_error(id);
        }
        self.connection_updated(id.clone(), None, NodeStatus::Disconnected);
        self.connected_peers.remove(id);
    }

    fn inject_replaced(&mut self, peer_id: PeerId, _old: ConnectedPoint, new_endpoint: ConnectedPoint) {
        // We need to re-send the active queries.
        for (query_id, query) in self.active_queries.iter() {
            if query.is_waiting(&peer_id) {
                self.queued_events.push(NetworkBehaviourAction::SendEvent {
                    peer_id: peer_id.clone(),
                    event: query.target().to_rpc_request(*query_id),
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
                self.queued_events.push(NetworkBehaviourAction::SendEvent {
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
                self.queued_events.push(NetworkBehaviourAction::SendEvent {
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
                if let Some(query) = self.active_queries.get_mut(&user_data) {
                    if let QueryInfoInner::GetProviders {
                        pending_results, ..
                    } = &mut query.target_mut().inner {
                        for peer in provider_peers {
                            pending_results.push(peer.node_id);
                        }
                    }
                }
            }
            KademliaHandlerEvent::QueryError { user_data, .. } => {
                // It is possible that we obtain a response for a query that has finished, which is
                // why we may not find an entry in `self.active_queries`.
                if let Some(query) = self.active_queries.get_mut(&user_data) {
                    query.on_failure(&source)
                }

                if let Some(write) = self.active_writes.get_mut(&user_data) {
                    write.inject_write_error(&source);
                }
            }
            KademliaHandlerEvent::AddProvider { key, provider_peer } => {
                self.queued_events.push(NetworkBehaviourAction::GenerateEvent(KademliaOut::Discovered {
                    peer_id: provider_peer.node_id.clone(),
                    addresses: provider_peer.multiaddrs.clone(),
                    ty: provider_peer.connection_ty,
                }));
                self.add_provider.push((key, provider_peer.node_id));
                return;
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

                self.queued_events.push(NetworkBehaviourAction::SendEvent {
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
                let mut finished_query = None;

                if let Some(query) = self.active_queries.get_mut(&user_data) {
                    if let QueryInfoInner::GetValue {
                        key: _,
                        results,
                        num_results,
                    } = &mut query.target_mut().inner {
                        if let Some(result) = result {
                            results.push(result);
                            if results.len() == *num_results {
                                finished_query = Some(user_data);
                            }
                        }
                    }
                }

                if let Some(finished_query) = finished_query {
                    let result = self.active_queries
                        .remove(&finished_query)
                        .expect("finished_query was gathered when peeking into active_queries; QED.")
                        .into_result();

                    match result.target.inner {
                        QueryInfoInner::GetValue { key: _, results, .. } => {
                            let result = GetValueResult::Found { results };
                            let event = KademliaOut::GetValueResult(result);

                            self.queued_events.push(NetworkBehaviourAction::GenerateEvent(event));
                        }
                        // TODO: write a better proof
                        _ => panic!("unexpected query_info.inner variant for a get_value result; QED.")
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
                    self.queued_events.push(NetworkBehaviourAction::SendEvent {
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
                if let Some(write) = self.active_writes.get_mut(&user_data) {
                    write.inject_write_success(&source);
                }
            }
        };
    }

    fn poll(
        &mut self,
        parameters: &mut impl PollParameters,
    ) -> Async<
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
                    self.start_query(QueryInfoInner::AddProvider { target });
                }
            },
            // Ignore errors.
            Ok(Async::Ready(None)) | Err(_) => {},
        }

        loop {
            // Drain queued events first.
            if !self.queued_events.is_empty() {
                return Async::Ready(self.queued_events.remove(0));
            }
            self.queued_events.shrink_to_fit();

            // Drain applied pending entries from the routing table.
            if let Some(entry) = self.kbuckets.take_applied_pending() {
                let event = KademliaOut::KBucketAdded {
                    peer_id: entry.inserted.into_preimage(),
                    replaced: entry.evicted.map(|n| n.key.into_preimage())
                };
                return Async::Ready(NetworkBehaviourAction::GenerateEvent(event))
            }

            // If iterating finds a query that is finished, stores it here and stops looping.
            let mut finished_query = None;

            'queries_iter: for (&query_id, query) in self.active_queries.iter_mut() {
                loop {
                    match query.next(now) {
                        QueryState::Finished => {
                            finished_query = Some(query_id);
                            break 'queries_iter;
                        }
                        QueryState::Waiting(Some(peer_id)) => {
                            if self.connected_peers.contains(peer_id) {
                                let peer_id = peer_id.clone();
                                let event = query.target().to_rpc_request(query_id);
                                return Async::Ready(NetworkBehaviourAction::SendEvent {
                                    peer_id,
                                    event
                                });
                            } else if peer_id != self.kbuckets.local_key().preimage() {
                                let peer_id = peer_id.clone();
                                let event = query.target().to_rpc_request(query_id);
                                self.pending_rpcs.push((peer_id.clone(), event));
                                return Async::Ready(NetworkBehaviourAction::DialPeer {
                                    peer_id,
                                });
                            }
                        }
                        QueryState::Waiting(None) | QueryState::WaitingAtCapacity => break,
                    }
                }
            }

            let finished_write =  self.active_writes.iter()
                .find_map(|(&query_id, write)|
                          if write.done() {
                              Some(query_id)
                          } else {
                              None
                          });

            if let Some(finished_write) = finished_write {
                let (t, successes, failures) = self
                    .active_writes
                    .remove(&finished_write)
                    .expect("finished_write was gathered when iterating active_writes; QED.")
                    .into_inner();
                let event = KademliaOut::PutValueResult(PutValueResult::Ok {
                    key: t,
                    successes,
                    failures,
                });

                break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
            }

            if let Some(finished_query) = finished_query {
                let result = self.active_queries
                    .remove(&finished_query)
                    .expect("finished_query was gathered when iterating active_queries; QED.")
                    .into_result();

                match result.target.inner {
                    QueryInfoInner::Initialization { .. } => {},
                    QueryInfoInner::FindPeer(target) => {
                        let event = KademliaOut::FindNodeResult {
                            key: target,
                            closer_peers: result.closest_peers.collect(),
                        };
                        break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    QueryInfoInner::GetProviders { target, pending_results } => {
                        let event = KademliaOut::GetProvidersResult {
                            key: target,
                            closer_peers: result.closest_peers.collect(),
                            provider_peers: pending_results,
                        };

                        break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    QueryInfoInner::AddProvider { target } => {
                        for closest in result.closest_peers {
                            let event = NetworkBehaviourAction::SendEvent {
                                peer_id: closest,
                                event: KademliaHandlerIn::AddProvider {
                                    key: target.clone(),
                                    provider_peer: KadPeer {
                                        node_id: parameters.local_peer_id().clone(),
                                        multiaddrs: parameters.external_addresses().collect(),
                                        connection_ty: KadConnectionType::Connected,
                                    }
                                },
                            };
                            self.queued_events.push(event);
                        }
                    },
                    QueryInfoInner::GetValue { key, results, .. } => {
                        let result = match results.len() {
                            0 => GetValueResult::NotFound {
                                key,
                                closest_peers: result.closest_peers.collect()
                            },
                            _ => GetValueResult::Found { results },
                        };

                        let event = KademliaOut::GetValueResult(result);

                        break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    QueryInfoInner::PutValue { key, value } => {
                        let closest_peers = Vec::from_iter(result.closest_peers);
                        for peer in &closest_peers {
                            let event = KademliaHandlerIn::PutValue {
                                key: key.clone(),
                                value: value.clone(),
                                user_data: finished_query,
                            };

                            if self.connected_peers.contains(peer) {
                                let event = NetworkBehaviourAction::SendEvent {
                                    peer_id: peer.clone(),
                                    event
                                };
                                self.queued_events.push(event);
                            } else {
                                self.pending_rpcs.push((peer.clone(), event));
                                self.queued_events.push(NetworkBehaviourAction::DialPeer {
                                    peer_id: peer.clone(),
                                });
                            }
                        }

                        self.active_writes.insert(finished_query, WriteState::new(key, closest_peers));
                    },
                }
            } else {
                break Async::NotReady;
            }
        }
    }
}

/// The result of a `GET_VALUE` query.
#[derive(Debug, Clone, PartialEq)]
pub enum GetValueResult {
    /// The results received from peers. Always contains non-zero number of results.
    Found { results: Vec<Record> },
    /// The record wasn't found.
    NotFound {
        key: Multihash,
        closest_peers: Vec<PeerId>
    }
}

/// The result of a `PUT_VALUE` query.
#[derive(Debug, Clone, PartialEq)]
pub enum PutValueResult {
    /// The value has been put successfully.
    Ok { key: Multihash, successes: usize, failures: usize },
    /// The put value failed.
    Err { key: Multihash, cause: RecordStorageError }
}

/// Output event of the `Kademlia` behaviour.
#[derive(Debug, Clone)]
pub enum KademliaOut {
    /// We have discovered a node.
    ///
    /// > **Note**: The Kademlia behaviour doesn't store the addresses of this node, and therefore
    /// >           attempting to connect to this node may or may not work.
    Discovered {
        /// Id of the node that was discovered.
        peer_id: PeerId,
        /// Addresses of the node.
        addresses: Vec<Multiaddr>,
        /// How the reporter is connected to the reported.
        ty: KadConnectionType,
    },

    /// A node has been added to a k-bucket.
    KBucketAdded {
        /// Id of the node that was added.
        peer_id: PeerId,
        /// If `Some`, this addition replaced the value that is inside the option.
        replaced: Option<PeerId>,
    },

    /// Result of a `FIND_NODE` iterative query.
    FindNodeResult {
        /// The key that we looked for in the query.
        key: PeerId,
        /// List of peers ordered from closest to furthest away.
        closer_peers: Vec<PeerId>,
    },

    /// Result of a `GET_PROVIDERS` iterative query.
    GetProvidersResult {
        /// The key that we looked for in the query.
        key: Multihash,
        /// The peers that are providing the requested key.
        provider_peers: Vec<PeerId>,
        /// List of peers ordered from closest to furthest away.
        closer_peers: Vec<PeerId>,
    },

    /// Result of a `GET_VALUE` query
    GetValueResult(GetValueResult),

    /// Result of a `PUT_VALUE` query
    PutValueResult(PutValueResult),
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

