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

use crate::handler::{KademliaHandler, KademliaHandlerEvent, KademliaHandlerIn, KademliaRequestId};
use crate::kbucket::{KBucketsTable, Update};
use crate::protocol::{KadConnectionType, KadPeer};
use crate::query::{QueryConfig, QueryState, QueryStatePollOut, QueryTarget};
use fnv::{FnvHashMap, FnvHashSet};
use futures::{prelude::*, stream};
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, Multiaddr, PeerId};
use multihash::Multihash;
use rand;
use smallvec::SmallVec;
use std::{cmp::Ordering, marker::PhantomData, time::Duration, time::Instant};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Interval;

/// Network behaviour that handles Kademlia.
pub struct Kademlia<TSubstream> {
    /// Storage for the nodes. Contains the known multiaddresses for this node.
    kbuckets: KBucketsTable<PeerId, SmallVec<[Multiaddr; 4]>>,

    /// All the iterative queries we are currently performing, with their ID. The last parameter
    /// is the list of accumulated providers for `GET_PROVIDERS` queries.
    active_queries: FnvHashMap<QueryId, (QueryState, QueryPurpose, Vec<PeerId>)>,

    /// List of queries to start once we are inside `poll()`.
    queries_to_starts: SmallVec<[(QueryId, QueryTarget, QueryPurpose); 8]>,

    /// List of peers the swarm is connected to.
    connected_peers: FnvHashSet<PeerId>,

    /// Contains a list of peer IDs which we are not connected to, and an RPC query to send to them
    /// once they connect.
    pending_rpcs: SmallVec<[(PeerId, KademliaHandlerIn<QueryId>); 8]>,

    /// Identifier for the next query that we start.
    next_query_id: QueryId,

    /// Requests received by a remote that we should fulfill as soon as possible.
    remote_requests: SmallVec<[(PeerId, KademliaRequestId, QueryTarget); 4]>,

    /// List of values and peers that are providing them.
    ///
    /// Our local peer ID can be in this container.
    // TODO: Note that in reality the value is a SHA-256 of the actual value (https://github.com/libp2p/rust-libp2p/issues/694)
    values_providers: FnvHashMap<Multihash, SmallVec<[PeerId; 20]>>,

    /// List of values that we are providing ourselves. Must be kept in sync with
    /// `values_providers`.
    providing_keys: FnvHashSet<Multihash>,

    /// Interval to send `ADD_PROVIDER` messages to everyone.
    refresh_add_providers: stream::Fuse<Interval>,

    /// `α` in the Kademlia reference papers. Designates the maximum number of queries that we
    /// perform in parallel.
    parallelism: usize,

    /// `k` in the Kademlia reference papers. Number of results in a find node query.
    num_results: usize,

    /// Timeout for each individual RPC query.
    rpc_timeout: Duration,

    /// Events to return when polling.
    queued_events: SmallVec<[NetworkBehaviourAction<KademliaHandlerIn<QueryId>, KademliaOut>; 32]>,

    /// List of providers to add to the topology as soon as we are in `poll()`.
    add_provider: SmallVec<[(Multihash, PeerId); 32]>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

/// Opaque type. Each query that we start gets a unique number.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct QueryId(usize);

/// Reason why we have this query in the list of queries.
#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryPurpose {
    /// The query was created for the Kademlia initialization process.
    Initialization,
    /// The user requested this query to be performed. It should be reported when finished.
    UserRequest,
    /// We should add an `ADD_PROVIDER` message to the peers of the outcome.
    AddProvider(Multihash),
}

impl<TSubstream> Kademlia<TSubstream> {
    /// Creates a `Kademlia`.
    #[inline]
    pub fn new(local_peer_id: PeerId) -> Self {
        Self::new_inner(local_peer_id, true)
    }

    /// Creates a `Kademlia`.
    ///
    /// Contrary to `new`, doesn't perform the initialization queries that store our local ID into
    /// the DHT.
    #[inline]
    pub fn without_init(local_peer_id: PeerId) -> Self {
        Self::new_inner(local_peer_id, false)
    }

    /// Adds a known address for the given `PeerId`.
    pub fn add_address(&mut self, peer_id: &PeerId, address: Multiaddr) {
        if let Some(list) = self.kbuckets.entry_mut(peer_id) {
            if list.iter().all(|a| *a != address) {
                list.push(address);
            }
        }
    }

    /// Inner implementation of the constructors.
    fn new_inner(local_peer_id: PeerId, initialize: bool) -> Self {
        let parallelism = 3;

        let mut behaviour = Kademlia {
            kbuckets: KBucketsTable::new(local_peer_id, Duration::from_secs(60)),   // TODO: constant
            queued_events: SmallVec::new(),
            queries_to_starts: SmallVec::new(),
            active_queries: Default::default(),
            connected_peers: Default::default(),
            pending_rpcs: SmallVec::with_capacity(parallelism),
            next_query_id: QueryId(0),
            remote_requests: SmallVec::new(),
            values_providers: FnvHashMap::default(),
            providing_keys: FnvHashSet::default(),
            refresh_add_providers: Interval::new_interval(Duration::from_secs(60)).fuse(),     // TODO: constant
            parallelism,
            num_results: 20,
            rpc_timeout: Duration::from_secs(8),
            add_provider: SmallVec::new(),
            marker: PhantomData,
        };

        if initialize {
            // As part of the initialization process, we start one `FIND_NODE` for each bit of the
            // possible range of peer IDs.
            for n in 0..256 {
                let peer_id = match gen_random_id(behaviour.kbuckets.my_id(), n) {
                    Ok(p) => p,
                    Err(()) => continue,
                };

                behaviour.start_query(QueryTarget::FindPeer(peer_id), QueryPurpose::Initialization);
            }
        }

        behaviour
    }

    /// Builds the answer to a request.
    fn build_result<TUserData>(&mut self, query: QueryTarget, request_id: KademliaRequestId, parameters: &mut PollParameters<'_>)
        -> KademliaHandlerIn<TUserData>
    {
        match query {
            QueryTarget::FindPeer(key) => {
                let closer_peers = self.kbuckets
                    .find_closest_with_self(&key)
                    .take(self.num_results)
                    .map(|peer_id| build_kad_peer(peer_id, parameters, &self.kbuckets, &self.connected_peers))
                    .collect();

                KademliaHandlerIn::FindNodeRes {
                    closer_peers,
                    request_id,
                }
            },
            QueryTarget::GetProviders(key) => {
                let closer_peers = self.kbuckets
                    .find_closest_with_self(&key)
                    .take(self.num_results)
                    .map(|peer_id| build_kad_peer(peer_id, parameters, &self.kbuckets, &self.connected_peers))
                    .collect();

                let provider_peers = self.values_providers
                    .get(&key)
                    .into_iter()
                    .flat_map(|peers| peers)
                    .map(|peer_id| build_kad_peer(peer_id.clone(), parameters, &self.kbuckets, &self.connected_peers))
                    .collect();

                KademliaHandlerIn::GetProvidersRes {
                    closer_peers,
                    provider_peers,
                    request_id,
                }
            },
        }
    }
}

impl<TSubstream> Kademlia<TSubstream> {
    /// Starts an iterative `FIND_NODE` request.
    ///
    /// This will eventually produce an event containing the nodes of the DHT closest to the
    /// requested `PeerId`.
    #[inline]
    pub fn find_node(&mut self, peer_id: PeerId) {
        self.start_query(QueryTarget::FindPeer(peer_id), QueryPurpose::UserRequest);
    }

    /// Starts an iterative `GET_PROVIDERS` request.
    #[inline]
    pub fn get_providers(&mut self, key: Multihash) {
        self.start_query(QueryTarget::GetProviders(key), QueryPurpose::UserRequest);
    }

    /// Register the local node as the provider for the given key.
    ///
    /// This will periodically send `ADD_PROVIDER` messages to the nodes closest to the key. When
    /// someone performs a `GET_PROVIDERS` iterative request on the DHT, our local node will be
    /// returned as part of the results.
    ///
    /// The actual meaning of *providing* the value of a key is not defined, and is specific to
    /// the value whose key is the hash.
    pub fn add_providing(&mut self, key: PeerId) {
        self.providing_keys.insert(key.clone().into());
        let providers = self.values_providers.entry(key.into()).or_insert_with(Default::default);
        let my_id = self.kbuckets.my_id();
        if !providers.iter().any(|k| k == my_id) {
            providers.push(my_id.clone());
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
    fn start_query(&mut self, target: QueryTarget, purpose: QueryPurpose) {
        let query_id = self.next_query_id;
        self.next_query_id.0 += 1;
        self.queries_to_starts.push((query_id, target, purpose));
    }
}

impl<TSubstream> NetworkBehaviour for Kademlia<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = KademliaHandler<TSubstream, QueryId>;
    type OutEvent = KademliaOut;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        KademliaHandler::dial_and_listen()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.kbuckets
            .get(peer_id)
            .map(|l| l.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_else(Vec::new)
    }

    fn inject_connected(&mut self, id: PeerId, endpoint: ConnectedPoint) {
        if let Some(pos) = self.pending_rpcs.iter().position(|(p, _)| p == &id) {
            let (_, rpc) = self.pending_rpcs.remove(pos);
            self.queued_events.push(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: rpc,
            });
        }

        match self.kbuckets.set_connected(&id) {
            Update::Pending(to_ping) => {
                self.queued_events.push(NetworkBehaviourAction::DialPeer {
                    peer_id: to_ping.clone(),
                })
            },
            _ => ()
        }

        if let ConnectedPoint::Dialer { address } = endpoint {
            if let Some(list) = self.kbuckets.entry_mut(&id) {
                if list.iter().all(|a| *a != address) {
                    list.push(address);
                }
            }
        }

        self.connected_peers.insert(id);
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        let was_in = self.connected_peers.remove(id);
        debug_assert!(was_in);

        for (query, _, _) in self.active_queries.values_mut() {
            query.inject_rpc_error(id);
        }

        self.kbuckets.set_disconnected(&id);
    }

    fn inject_replaced(&mut self, peer_id: PeerId, _: ConnectedPoint, new_endpoint: ConnectedPoint) {
        // We need to re-send the active queries.
        for (query_id, (query, _, _)) in self.active_queries.iter() {
            if query.is_waiting(&peer_id) {
                self.queued_events.push(NetworkBehaviourAction::SendEvent {
                    peer_id: peer_id.clone(),
                    event: query.target().to_rpc_request(*query_id),
                });
            }
        }

        if let ConnectedPoint::Dialer { address } = new_endpoint {
            if let Some(list) = self.kbuckets.entry_mut(&peer_id) {
                if list.iter().all(|a| *a != address) {
                    list.push(address);
                }
            }
        }
    }

    fn inject_node_event(&mut self, source: PeerId, event: KademliaHandlerEvent<QueryId>) {
        match event {
            KademliaHandlerEvent::FindNodeReq { key, request_id } => {
                self.remote_requests.push((source, request_id, QueryTarget::FindPeer(key)));
                return;
            }
            KademliaHandlerEvent::FindNodeRes {
                closer_peers,
                user_data,
            } => {
                // It is possible that we obtain a response for a query that has finished, which is
                // why we may not find an entry in `self.active_queries`.
                for peer in closer_peers.iter() {
                    if let Some(entry) = self.kbuckets.entry_mut(&peer.node_id) {
                        for addr in peer.multiaddrs.iter() {
                            if entry.iter().all(|a| a != addr) {
                                entry.push(addr.clone());
                            }
                        }
                    }

                    self.queued_events.push(NetworkBehaviourAction::GenerateEvent(KademliaOut::Discovered {
                        peer_id: peer.node_id.clone(),
                        addresses: peer.multiaddrs.clone(),
                        ty: peer.connection_ty,
                    }));
                }
                if let Some((query, _, _)) = self.active_queries.get_mut(&user_data) {
                    query.inject_rpc_result(&source, closer_peers.into_iter().map(|kp| kp.node_id))
                }
            }
            KademliaHandlerEvent::GetProvidersReq { key, request_id } => {
                self.remote_requests.push((source, request_id, QueryTarget::GetProviders(key)));
                return;
            }
            KademliaHandlerEvent::GetProvidersRes {
                closer_peers,
                provider_peers,
                user_data,
            } => {
                for peer in closer_peers.iter().chain(provider_peers.iter()) {
                    if let Some(entry) = self.kbuckets.entry_mut(&peer.node_id) {
                        for addr in peer.multiaddrs.iter() {
                            if entry.iter().all(|a| a != addr) {
                                entry.push(addr.clone());
                            }
                        }
                    }

                    self.queued_events.push(NetworkBehaviourAction::GenerateEvent(KademliaOut::Discovered {
                        peer_id: peer.node_id.clone(),
                        addresses: peer.multiaddrs.clone(),
                        ty: peer.connection_ty,
                    }));
                }

                // It is possible that we obtain a response for a query that has finished, which is
                // why we may not find an entry in `self.active_queries`.
                if let Some((query, _, providers)) = self.active_queries.get_mut(&user_data) {
                    for peer in provider_peers {
                        providers.push(peer.node_id);
                    }
                    query.inject_rpc_result(&source, closer_peers.into_iter().map(|kp| kp.node_id))
                }
            }
            KademliaHandlerEvent::QueryError { user_data, .. } => {
                // It is possible that we obtain a response for a query that has finished, which is
                // why we may not find an entry in `self.active_queries`.
                if let Some((query, _, _)) = self.active_queries.get_mut(&user_data) {
                    query.inject_rpc_error(&source)
                }
            }
            KademliaHandlerEvent::AddProvider { key, provider_peer } => {
                if let Some(entry) = self.kbuckets.entry_mut(&provider_peer.node_id) {
                    for addr in provider_peer.multiaddrs.iter() {
                        if entry.iter().all(|a| a != addr) {
                            entry.push(addr.clone());
                        }
                    }
                }
                self.queued_events.push(NetworkBehaviourAction::GenerateEvent(KademliaOut::Discovered {
                    peer_id: provider_peer.node_id.clone(),
                    addresses: provider_peer.multiaddrs.clone(),
                    ty: provider_peer.connection_ty,
                }));
                self.add_provider.push((key, provider_peer.node_id));
                return;
            }
        };
    }

    fn poll(
        &mut self,
        parameters: &mut PollParameters<'_>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Flush the changes to the topology that we want to make.
        for (key, provider) in self.add_provider.drain() {
            // Don't add ourselves to the providers.
            if provider == *self.kbuckets.my_id() {
                continue;
            }
            let providers = self.values_providers.entry(key).or_insert_with(Default::default);
            if !providers.iter().any(|k| k == &provider) {
                providers.push(provider);
            }
        }
        self.add_provider.shrink_to_fit();

        // Handle `refresh_add_providers`.
        match self.refresh_add_providers.poll() {
            Ok(Async::NotReady) => {},
            Ok(Async::Ready(Some(_))) => {
                for provided in self.providing_keys.clone().into_iter() {
                    let purpose = QueryPurpose::AddProvider(provided.clone());
                    // TODO: messy because of the PeerId/Multihash division
                    if let Ok(key_as_peer) = PeerId::from_multihash(provided) {
                        self.start_query(QueryTarget::FindPeer(key_as_peer), purpose);
                    }
                }
            },
            // Ignore errors.
            Ok(Async::Ready(None)) | Err(_) => {},
        }

        // Start queries that are waiting to start.
        for (query_id, query_target, query_purpose) in self.queries_to_starts.drain() {
            let known_closest_peers = self.kbuckets
                .find_closest(query_target.as_hash())
                .take(self.num_results);
            self.active_queries.insert(
                query_id,
                (
                    QueryState::new(QueryConfig {
                        target: query_target,
                        parallelism: self.parallelism,
                        num_results: self.num_results,
                        rpc_timeout: self.rpc_timeout,
                        known_closest_peers,
                    }),
                    query_purpose,
                    Vec::new()      // TODO: insert ourselves if we provide the data?
                )
            );
        }
        self.queries_to_starts.shrink_to_fit();

        // Handle remote queries.
        if !self.remote_requests.is_empty() {
            let (peer_id, request_id, query) = self.remote_requests.remove(0);
            let result = self.build_result(query, request_id, parameters);
            return Async::Ready(NetworkBehaviourAction::SendEvent {
                peer_id,
                event: result,
            });
        }

        loop {
            // Handle events queued by other parts of this struct
            if !self.queued_events.is_empty() {
                return Async::Ready(self.queued_events.remove(0));
            }
            self.queued_events.shrink_to_fit();

            // If iterating finds a query that is finished, stores it here and stops looping.
            let mut finished_query = None;

            'queries_iter: for (&query_id, (query, _, _)) in self.active_queries.iter_mut() {
                loop {
                    match query.poll() {
                        Async::Ready(QueryStatePollOut::Finished) => {
                            finished_query = Some(query_id);
                            break 'queries_iter;
                        }
                        Async::Ready(QueryStatePollOut::SendRpc {
                            peer_id,
                            query_target,
                        }) => {
                            let rpc = query_target.to_rpc_request(query_id);
                            if self.connected_peers.contains(&peer_id) {
                                return Async::Ready(NetworkBehaviourAction::SendEvent {
                                    peer_id: peer_id.clone(),
                                    event: rpc,
                                });
                            } else {
                                self.pending_rpcs.push((peer_id.clone(), rpc));
                                return Async::Ready(NetworkBehaviourAction::DialPeer {
                                    peer_id: peer_id.clone(),
                                });
                            }
                        }
                        Async::Ready(QueryStatePollOut::CancelRpc { peer_id }) => {
                            // We don't cancel if the RPC has already been sent out.
                            self.pending_rpcs.retain(|(id, _)| id != peer_id);
                        }
                        Async::NotReady => break,
                    }
                }
            }

            if let Some(finished_query) = finished_query {
                let (query, purpose, provider_peers) = self
                    .active_queries
                    .remove(&finished_query)
                    .expect("finished_query was gathered when iterating active_queries; QED.");
                match purpose {
                    QueryPurpose::Initialization => {},
                    QueryPurpose::UserRequest => {
                        let event = match query.target().clone() {
                            QueryTarget::FindPeer(key) => {
                                debug_assert!(provider_peers.is_empty());
                                KademliaOut::FindNodeResult {
                                    key,
                                    closer_peers: query.into_closest_peers().collect(),
                                }
                            },
                            QueryTarget::GetProviders(key) => {
                                KademliaOut::GetProvidersResult {
                                    key,
                                    closer_peers: query.into_closest_peers().collect(),
                                    provider_peers,
                                }
                            },
                        };

                        break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    QueryPurpose::AddProvider(key) => {
                        for closest in query.into_closest_peers() {
                            let event = NetworkBehaviourAction::SendEvent {
                                peer_id: closest,
                                event: KademliaHandlerIn::AddProvider {
                                    key: key.clone(),
                                    provider_peer: build_kad_peer(parameters.local_peer_id().clone(), parameters, &self.kbuckets, &self.connected_peers),
                                },
                            };

                            self.queued_events.push(event);
                        }
                    },
                }
            } else {
                break Async::NotReady;
            }
        }
    }
}

/// Output event of the `Kademlia` behaviour.
#[derive(Debug, Clone)]
pub enum KademliaOut {
    /// We have discovered a node.
    Discovered {
        /// Id of the node that was discovered.
        peer_id: PeerId,
        /// Addresses of the node.
        addresses: Vec<Multiaddr>,
        /// How the reporter is connected to the reported.
        ty: KadConnectionType,
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
}

// Generates a random `PeerId` that belongs to the given bucket.
//
// Returns an error if `bucket_num` is out of range.
fn gen_random_id(my_id: &PeerId, bucket_num: usize) -> Result<PeerId, ()> {
    let my_id_len = my_id.as_bytes().len();

    // TODO: this 2 is magic here; it is the length of the hash of the multihash
    let bits_diff = bucket_num + 1;
    if bits_diff > 8 * (my_id_len - 2) {
        return Err(());
    }

    let mut random_id = [0; 64];
    for byte in 0..my_id_len {
        match byte.cmp(&(my_id_len - bits_diff / 8 - 1)) {
            Ordering::Less => {
                random_id[byte] = my_id.as_bytes()[byte];
            }
            Ordering::Equal => {
                let mask: u8 = (1 << (bits_diff % 8)) - 1;
                random_id[byte] = (my_id.as_bytes()[byte] & !mask) | (rand::random::<u8>() & mask);
            }
            Ordering::Greater => {
                random_id[byte] = rand::random();
            }
        }
    }

    let peer_id = PeerId::from_bytes(random_id[..my_id_len].to_owned())
        .expect("randomly-generated peer ID should always be valid");
    Ok(peer_id)
}

/// Builds a `KadPeer` struct corresponding to the given `PeerId`.
/// The `PeerId` can be the same as the local one.
///
/// > **Note**: This is just a convenience function that doesn't do anything note-worthy.
fn build_kad_peer(
    peer_id: PeerId,
    parameters: &mut PollParameters<'_>,
    kbuckets: &KBucketsTable<PeerId, SmallVec<[Multiaddr; 4]>>,
    connected_peers: &FnvHashSet<PeerId>
) -> KadPeer {
    let is_self = peer_id == *parameters.local_peer_id();

    let multiaddrs = if is_self {
        let mut addrs = parameters
            .listened_addresses()
            .cloned()
            .collect::<Vec<_>>();
        addrs.extend(parameters.external_addresses());
        addrs
    } else {
        kbuckets
            .get(&peer_id)
            .map(|addrs| addrs.iter().cloned().collect())
            .unwrap_or_else(Vec::new)
    };

    // TODO: implement the other possibilities correctly
    let connection_ty = if is_self || connected_peers.contains(&peer_id) {
        KadConnectionType::Connected
    } else {
        KadConnectionType::NotConnected
    };

    KadPeer {
        node_id: peer_id,
        multiaddrs,
        connection_ty,
    }
}
