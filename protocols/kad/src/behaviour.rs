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

use fnv::{FnvHashMap, FnvHashSet};
use futures::{prelude::*, stream};
use handler::{KademliaHandler, KademliaHandlerEvent, KademliaHandlerIn, KademliaRequestId};
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, topology::Topology, Multiaddr, PeerId};
use multihash::Multihash;
use protocol::{KadConnectionType, KadPeer};
use query::{QueryConfig, QueryState, QueryStatePollOut, QueryTarget};
use rand;
use smallvec::SmallVec;
use std::{cmp::Ordering, marker::PhantomData, time::Duration, time::Instant};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Interval;
use topology::KademliaTopology;

/// Network behaviour that handles Kademlia.
pub struct Kademlia<TSubstream> {
    /// Peer ID of the local node.
    local_peer_id: PeerId,

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

    /// List of multihashes that we're providing.
    ///
    /// Note that we use a `PeerId` so that we know that it uses SHA-256. The question as to how to
    /// handle more hashes should eventually be resolved.
    providing_keys: SmallVec<[PeerId; 8]>,

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

    /// List of addresses to add to the topology as soon as we are in `poll()`.
    add_to_topology: SmallVec<[(PeerId, Multiaddr, KadConnectionType); 32]>,

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

    /// Inner implementation of the constructors.
    fn new_inner(local_peer_id: PeerId, initialize: bool) -> Self {
        let parallelism = 3;

        let mut behaviour = Kademlia {
            local_peer_id: local_peer_id.clone(),
            queued_events: SmallVec::new(),
            queries_to_starts: SmallVec::new(),
            active_queries: Default::default(),
            connected_peers: Default::default(),
            pending_rpcs: SmallVec::with_capacity(parallelism),
            next_query_id: QueryId(0),
            remote_requests: SmallVec::new(),
            providing_keys: SmallVec::new(),
            refresh_add_providers: Interval::new_interval(Duration::from_secs(60)).fuse(),     // TODO: constant
            parallelism,
            num_results: 20,
            rpc_timeout: Duration::from_secs(8),
            add_to_topology: SmallVec::new(),
            add_provider: SmallVec::new(),
            marker: PhantomData,
        };

        if initialize {
            // As part of the initialization process, we start one `FIND_NODE` for each bit of the
            // possible range of peer IDs.
            for n in 0..256 {
                let peer_id = match gen_random_id(&local_peer_id, n) {
                    Ok(p) => p,
                    Err(()) => continue,
                };

                behaviour.start_query(QueryTarget::FindPeer(peer_id), QueryPurpose::Initialization);
            }
        }

        behaviour
    }

    /// Builds a `KadPeer` structure corresponding to the local node.
    fn build_local_kad_peer<'a>(&self, local_addrs: impl IntoIterator<Item = &'a Multiaddr>) -> KadPeer {
        KadPeer {
            node_id: self.local_peer_id.clone(),
            multiaddrs: local_addrs.into_iter().cloned().collect(),
            connection_ty: KadConnectionType::Connected,
        }
    }

    /// Builds the answer to a request.
    fn build_result<TUserData, TTopology>(&self, query: QueryTarget, request_id: KademliaRequestId, parameters: &mut PollParameters<TTopology>)
        -> KademliaHandlerIn<TUserData>
    where TTopology: KademliaTopology
    {
        let local_kad_peer = self.build_local_kad_peer(parameters.external_addresses());

        match query {
            QueryTarget::FindPeer(key) => {
                let mut topology = parameters.topology();
                // TODO: insert local_kad_peer somewhere?
                let closer_peers = topology
                    .closest_peers(key.as_ref(), self.num_results)
                    .map(|peer_id| build_kad_peer(peer_id, topology, &self.connected_peers))
                    .collect();

                KademliaHandlerIn::FindNodeRes {
                    closer_peers,
                    request_id,
                }
            },
            QueryTarget::GetProviders(key) => {
                let mut topology = parameters.topology();
                // TODO: insert local_kad_peer somewhere?
                let closer_peers = topology
                    .closest_peers(&key, self.num_results)
                    .map(|peer_id| build_kad_peer(peer_id, topology, &self.connected_peers))
                    .collect();

                let local_node_is_providing = self.providing_keys.iter().any(|k| k.as_ref() == &key);

                let provider_peers = topology
                    .get_providers(&key)
                    .map(|peer_id| build_kad_peer(peer_id, topology, &self.connected_peers))
                    .chain(if local_node_is_providing {
                        Some(local_kad_peer)
                    } else {
                        None
                    }.into_iter())
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
        if !self.providing_keys.iter().any(|k| k == &key) {
            self.providing_keys.push(key);
        }

        // Trigger the next refresh now.
        self.refresh_add_providers = Interval::new(Instant::now(), Duration::from_secs(60)).fuse();
    }

    /// Cancels a registration done with `add_providing`.
    ///
    /// There doesn't exist any "remove provider" message to broadcast on the network, therefore we
    /// will still be registered as a provider in the DHT for as long as the timeout doesn't expire.
    pub fn remove_providing(&mut self, key: &Multihash) {
        if let Some(position) = self.providing_keys.iter().position(|k| k.as_ref() == key) {
            self.providing_keys.remove(position);
        }
    }

    /// Internal function that starts a query.
    fn start_query(&mut self, target: QueryTarget, purpose: QueryPurpose) {
        let query_id = self.next_query_id.clone();
        self.next_query_id.0 += 1;
        self.queries_to_starts.push((query_id, target, purpose));
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for Kademlia<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
    TTopology: KademliaTopology,
{
    type ProtocolsHandler = KademliaHandler<TSubstream, QueryId>;
    type OutEvent = KademliaOut;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        KademliaHandler::dial_and_listen()
    }

    fn inject_connected(&mut self, id: PeerId, _: ConnectedPoint) {
        if let Some(pos) = self.pending_rpcs.iter().position(|(p, _)| p == &id) {
            let (_, rpc) = self.pending_rpcs.remove(pos);
            self.queued_events.push(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: rpc,
            });
        }

        self.connected_peers.insert(id);
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        let was_in = self.connected_peers.remove(id);
        debug_assert!(was_in);

        for (query, _, _) in self.active_queries.values_mut() {
            query.inject_rpc_error(id);
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
                    for addr in peer.multiaddrs.iter() {
                        self.add_to_topology
                            .push((peer.node_id.clone(), addr.clone(), peer.connection_ty));
                    }
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
                    for addr in peer.multiaddrs.iter() {
                        self.add_to_topology
                            .push((peer.node_id.clone(), addr.clone(), peer.connection_ty));
                    }
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
                for addr in provider_peer.multiaddrs.iter() {
                    self.add_to_topology
                        .push((provider_peer.node_id.clone(), addr.clone(), provider_peer.connection_ty));
                }
                self.add_provider.push((key, provider_peer.node_id));
                return;
            }
        };
    }

    fn poll(
        &mut self,
        parameters: &mut PollParameters<TTopology>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Flush the changes to the topology that we want to make.
        for (peer_id, addr, connection_ty) in self.add_to_topology.drain() {
            parameters.topology().add_kad_discovered_address(peer_id, addr, connection_ty);
        }
        self.add_to_topology.shrink_to_fit();
        for (key, provider) in self.add_provider.drain() {
            parameters.topology().add_provider(key, provider);
        }
        self.add_provider.shrink_to_fit();

        // Handle `refresh_add_providers`.
        match self.refresh_add_providers.poll() {
            Ok(Async::NotReady) => {},
            Ok(Async::Ready(Some(_))) => {
                for provided in self.providing_keys.clone().into_iter() {
                    let purpose = QueryPurpose::AddProvider(provided.as_ref().clone());
                    self.start_query(QueryTarget::FindPeer(provided), purpose);
                }
            },
            // Ignore errors.
            Ok(Async::Ready(None)) | Err(_) => {},
        }

        // Start queries that are waiting to start.
        for (query_id, query_target, query_purpose) in self.queries_to_starts.drain() {
            let known_closest_peers = parameters
                .topology()
                .closest_peers(query_target.as_hash(), self.num_results);
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
                    .expect("finished_query was gathered when iterating active_queries ; qed");
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
                                    provider_peer: self.build_local_kad_peer(parameters.external_addresses()),
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
///
/// > **Note**: This is just a convenience function that doesn't do anything note-worthy.
fn build_kad_peer<TTopology>(peer_id: PeerId, topology: &mut TTopology, connected_peers: &FnvHashSet<PeerId>) -> KadPeer
where TTopology: Topology
{
    let multiaddrs = topology.addresses_of_peer(&peer_id);

    // TODO: implement the other possibilities correctly
    let connection_ty = if connected_peers.contains(&peer_id) {
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
