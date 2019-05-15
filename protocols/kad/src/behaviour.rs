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
use crate::kad_hash::KadHash;
use crate::kbucket::{self, KBucketsTable, KBucketsPeerId};
use crate::protocol::{KadConnectionType, KadPeer};
use crate::query::{QueryConfig, QueryState, QueryStatePollOut};
use fnv::{FnvHashMap, FnvHashSet};
use futures::{prelude::*, stream};
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, Multiaddr, PeerId};
use multihash::Multihash;
use smallvec::SmallVec;
use std::{borrow::Cow, error, marker::PhantomData, num::NonZeroUsize, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::{Instant, Interval};

mod test;

/// Network behaviour that handles Kademlia.
pub struct Kademlia<TSubstream> {
    /// Storage for the nodes. Contains the known multiaddresses for this node.
    kbuckets: KBucketsTable<KadHash, Addresses>,

    /// If `Some`, we overwrite the Kademlia protocol name with this one.
    protocol_name_override: Option<Cow<'static, [u8]>>,

    /// All the iterative queries we are currently performing, with their ID. The last parameter
    /// is the list of accumulated providers for `GET_PROVIDERS` queries.
    active_queries: FnvHashMap<QueryId, QueryState<QueryInfo, PeerId>>,

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
}

impl KBucketsPeerId<PeerId> for QueryInfo {
    fn distance_with(&self, other: &PeerId) -> u32 {
        let other: &Multihash = other.as_ref();
        self.as_ref().distance_with(other)
    }

    fn max_distance() -> NonZeroUsize {
        <PeerId as KBucketsPeerId>::max_distance()
    }
}

impl AsRef<Multihash> for QueryInfo {
    fn as_ref(&self) -> &Multihash {
        match &self.inner {
            QueryInfoInner::Initialization { target } => target.as_ref(),
            QueryInfoInner::FindPeer(peer) => peer.as_ref(),
            QueryInfoInner::GetProviders { target, .. } => target,
            QueryInfoInner::AddProvider { target } => target,
        }
    }
}

impl PartialEq<PeerId> for QueryInfo {
    fn eq(&self, other: &PeerId) -> bool {
        self.as_ref().eq(other)
    }
}

impl QueryInfo {
    /// Creates the corresponding RPC request to send to remote.
    fn to_rpc_request<TUserData>(&self, user_data: TUserData) -> KademliaHandlerIn<TUserData> {
        match &self.inner {
            QueryInfoInner::Initialization { target } => KademliaHandlerIn::FindNodeReq {
                key: target.clone(),
                user_data,
            },
            QueryInfoInner::FindPeer(key) => KademliaHandlerIn::FindNodeReq {
                key: key.clone(),
                user_data,
            },
            QueryInfoInner::GetProviders { target, .. } => KademliaHandlerIn::GetProvidersReq {
                key: target.clone().into(),
                user_data,
            },
            QueryInfoInner::AddProvider { .. } => KademliaHandlerIn::FindNodeReq {
                key: unimplemented!(), // TODO: target.clone(),
                user_data,
            },
        }
    }
}

impl<TSubstream> Kademlia<TSubstream> {
    /// Creates a `Kademlia`.
    #[inline]
    pub fn new(local_peer_id: PeerId) -> Self {
        Self::new_inner(local_peer_id)
    }

    /// The same as `new`, but using a custom protocol name.
    ///
    /// Kademlia nodes only communicate with other nodes using the same protocol name. Using a
    /// custom name therefore allows to segregate the DHT from others, if that is desired.
    pub fn with_protocol_name(local_peer_id: PeerId, name: impl Into<Cow<'static, [u8]>>) -> Self {
        let mut me = Kademlia::new_inner(local_peer_id);
        me.protocol_name_override = Some(name.into());
        me
    }

    /// Creates a `Kademlia`.
    ///
    /// Contrary to `new`, doesn't perform the initialization queries that store our local ID into
    /// the DHT and fill our buckets.
    #[inline]
    #[deprecated(note="this function is now equivalent to new() and will be removed in the future")]
    pub fn without_init(local_peer_id: PeerId) -> Self {
        Self::new_inner(local_peer_id)
    }

    /// Adds a known address for the given `PeerId`. We are connected to this address.
    // TODO: report if the address was inserted? also, semantics unclear
    pub fn add_connected_address(&mut self, peer_id: &PeerId, address: Multiaddr) {
        self.add_address(peer_id, address, true)
    }

    /// Adds a known address for the given `PeerId`. We are not connected or don't know whether we
    /// are connected to this address.
    // TODO: report if the address was inserted? also, semantics unclear
    pub fn add_not_connected_address(&mut self, peer_id: &PeerId, address: Multiaddr) {
        self.add_address(peer_id, address, false)
    }

    /// Underlying implementation for `add_connected_address` and `add_not_connected_address`.
    fn add_address(&mut self, peer_id: &PeerId, address: Multiaddr, _connected: bool) {
        let kad_hash = KadHash::from(peer_id.clone());

        match self.kbuckets.entry(&kad_hash) {
            kbucket::Entry::InKbucketConnected(mut entry) => entry.value().insert(address),
            kbucket::Entry::InKbucketConnectedPending(mut entry) => entry.value().insert(address),
            kbucket::Entry::InKbucketDisconnected(mut entry) => entry.value().insert(address),
            kbucket::Entry::InKbucketDisconnectedPending(mut entry) => entry.value().insert(address),
            kbucket::Entry::NotInKbucket(entry) => {
                let mut addresses = Addresses::new();
                addresses.insert(address);
                match entry.insert_disconnected(addresses) {
                    kbucket::InsertOutcome::Inserted => {
                        let event = KademliaOut::KBucketAdded {
                            peer_id: peer_id.clone(),
                            replaced: None,
                        };
                        self.queued_events.push(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    kbucket::InsertOutcome::Full => (),
                    kbucket::InsertOutcome::Pending { to_ping } => {
                        self.queued_events.push(NetworkBehaviourAction::DialPeer {
                            peer_id: to_ping.peer_id().clone(),
                        })
                    },
                }
                return;
            },
            kbucket::Entry::SelfEntry => return,
        };
    }

    /// Inner implementation of the constructors.
    fn new_inner(local_peer_id: PeerId) -> Self {
        let parallelism = 3;

        Kademlia {
            kbuckets: KBucketsTable::new(local_peer_id.into(), Duration::from_secs(60)),   // TODO: constant
            protocol_name_override: None,
            queued_events: SmallVec::new(),
            active_queries: Default::default(),
            connected_peers: Default::default(),
            pending_rpcs: SmallVec::with_capacity(parallelism),
            next_query_id: QueryId(0),
            values_providers: FnvHashMap::default(),
            providing_keys: FnvHashSet::default(),
            refresh_add_providers: Interval::new_interval(Duration::from_secs(60)).fuse(),     // TODO: constant
            parallelism,
            num_results: 20,
            rpc_timeout: Duration::from_secs(8),
            add_provider: SmallVec::new(),
            marker: PhantomData,
        }
    }

    /// Returns an iterator to all the peer IDs in the bucket, without the pending nodes.
    pub fn kbuckets_entries(&self) -> impl Iterator<Item = &PeerId> {
        self.kbuckets.entries_not_pending().map(|(kad_hash, _)| kad_hash.peer_id())
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
        let my_id = self.kbuckets.my_id();
        if !providers.iter().any(|peer_id| peer_id == my_id.peer_id()) {
            providers.push(my_id.peer_id().clone());
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

        let known_closest_peers = self.kbuckets
            .find_closest(target.as_ref())
            .take(self.num_results)
            .map(|h| h.peer_id().clone());

        self.active_queries.insert(
            query_id,
            QueryState::new(QueryConfig {
                target,
                parallelism: self.parallelism,
                num_results: self.num_results,
                rpc_timeout: self.rpc_timeout,
                known_closest_peers,
            })
        );
    }

    /// Processes discovered peers from a query.
    fn discovered<'a, I>(&'a mut self, query_id: &QueryId, source: &PeerId, peers: I)
    where
        I: Iterator<Item=&'a KadPeer> + Clone
    {
        let local_id = self.kbuckets.my_id().peer_id().clone();
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
            query.inject_rpc_result(source, others_iter.cloned().map(|kp| kp.node_id))
        }
    }
}

impl<TSubstream> NetworkBehaviour for Kademlia<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
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
        let mut out_list = self.kbuckets
            .entry(&KadHash::from(peer_id.clone()))
            .value_not_pending()
            .map(|l| l.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_else(Vec::new);

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
        if let Some(pos) = self.pending_rpcs.iter().position(|(p, _)| p == &id) {
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

        let id_kad_hash = KadHash::from(id.clone());

        match self.kbuckets.entry(&id_kad_hash) {
            kbucket::Entry::InKbucketConnected(_) => {
                unreachable!("Kbuckets are always kept in sync with the connection state; QED")
            },
            kbucket::Entry::InKbucketConnectedPending(_) => {
                unreachable!("Kbuckets are always kept in sync with the connection state; QED")
            },

            kbucket::Entry::InKbucketDisconnected(mut entry) => {
                if let Some(address) = address {
                    entry.value().insert(address);
                }
                entry.set_connected();
            },

            kbucket::Entry::InKbucketDisconnectedPending(mut entry) => {
                if let Some(address) = address {
                    entry.value().insert(address);
                }
                entry.set_connected();
            },

            kbucket::Entry::NotInKbucket(entry) => {
                let mut addresses = Addresses::new();
                if let Some(address) = address {
                    addresses.insert(address);
                }
                match entry.insert_connected(addresses) {
                    kbucket::InsertOutcome::Inserted => {
                        let event = KademliaOut::KBucketAdded {
                            peer_id: id.clone(),
                            replaced: None,
                        };
                        self.queued_events.push(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    kbucket::InsertOutcome::Full => (),
                    kbucket::InsertOutcome::Pending { to_ping } => {
                        self.queued_events.push(NetworkBehaviourAction::DialPeer {
                            peer_id: to_ping.peer_id().clone(),
                        })
                    },
                }
            },

            kbucket::Entry::SelfEntry => {
                unreachable!("Guaranteed to never receive disconnected even for self; QED")
            },
        }

        self.connected_peers.insert(id);
    }

    fn inject_addr_reach_failure(&mut self, peer_id: Option<&PeerId>, addr: &Multiaddr, _: &dyn error::Error) {
        if let Some(peer_id) = peer_id {
            let id_kad_hash = KadHash::from(peer_id.clone());

            if let Some(list) = self.kbuckets.entry(&id_kad_hash).value() {
                // TODO: don't remove the address if the error is that we are already connected
                //       to this peer
                list.remove(addr);
            }

            for query in self.active_queries.values_mut() {
                if let Some(addrs) = query.target_mut().untrusted_addresses.get_mut(id_kad_hash.peer_id()) {
                    addrs.retain(|a| a != addr);
                }
            }
        }
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        for query in self.active_queries.values_mut() {
            query.inject_rpc_error(peer_id);
        }
    }

    fn inject_disconnected(&mut self, id: &PeerId, _old_endpoint: ConnectedPoint) {
        let was_in = self.connected_peers.remove(id);
        debug_assert!(was_in);

        for query in self.active_queries.values_mut() {
            query.inject_rpc_error(id);
        }

        match self.kbuckets.entry(&KadHash::from(id.clone())) {
            kbucket::Entry::InKbucketConnected(entry) => {
                match entry.set_disconnected() {
                    kbucket::SetDisconnectedOutcome::Kept(_) => {},
                    kbucket::SetDisconnectedOutcome::Replaced { replacement, .. } => {
                        let event = KademliaOut::KBucketAdded {
                            peer_id: replacement.peer_id().clone(),
                            replaced: Some(id.clone()),
                        };
                        self.queued_events.push(NetworkBehaviourAction::GenerateEvent(event));
                    },
                }
            },
            kbucket::Entry::InKbucketConnectedPending(entry) => {
                entry.set_disconnected();
            },
            kbucket::Entry::InKbucketDisconnected(_) => {
                unreachable!("Kbuckets are always kept in sync with the connection state; QED")
            },
            kbucket::Entry::InKbucketDisconnectedPending(_) => {
                unreachable!("Kbuckets are always kept in sync with the connection state; QED")
            },
            kbucket::Entry::NotInKbucket(_) => {},
            kbucket::Entry::SelfEntry => {
                unreachable!("Guaranteed to never receive disconnected even for self; QED")
            },
        }
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

        if let Some(list) = self.kbuckets.entry(&KadHash::from(peer_id)).value() {
            if let ConnectedPoint::Dialer { address } = new_endpoint {
                list.insert(address);
            }
        }
    }

    fn inject_node_event(&mut self, source: PeerId, event: KademliaHandlerEvent<QueryId>) {
        match event {
            KademliaHandlerEvent::FindNodeReq { key, request_id } => {
                let closer_peers = self.kbuckets
                    .find_closest(&KadHash::from(key.clone()))
                    .filter(|p| p.peer_id() != &source)
                    .take(self.num_results)
                    .map(|kad_hash| build_kad_peer(&kad_hash, &mut self.kbuckets))
                    .collect();

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
                let closer_peers = self.kbuckets
                    .find_closest(&key)
                    .filter(|p| p.peer_id() != &source)
                    .take(self.num_results)
                    .map(|kad_hash| build_kad_peer(&kad_hash, &mut self.kbuckets))
                    .collect();

                let provider_peers = {
                    let kbuckets = &mut self.kbuckets;
                    self.values_providers
                        .get(&key)
                        .into_iter()
                        .flat_map(|peers| peers)
                        .filter(|p| *p != &source)
                        .map(move |peer_id| build_kad_peer(&KadHash::from(peer_id.clone()), kbuckets))
                        .collect()
                };

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
                    query.inject_rpc_error(&source)
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
            if provider == *self.kbuckets.my_id().peer_id() {
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
            // Handle events queued by other parts of this struct
            if !self.queued_events.is_empty() {
                return Async::Ready(self.queued_events.remove(0));
            }
            self.queued_events.shrink_to_fit();

            // If iterating finds a query that is finished, stores it here and stops looping.
            let mut finished_query = None;

            'queries_iter: for (&query_id, query) in self.active_queries.iter_mut() {
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
                            } else if peer_id != self.kbuckets.my_id().peer_id() {
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
                let (query_info, closer_peers) = self
                    .active_queries
                    .remove(&finished_query)
                    .expect("finished_query was gathered when iterating active_queries; QED.")
                    .into_target_and_closest_peers();

                match query_info.inner {
                    QueryInfoInner::Initialization { .. } => {},
                    QueryInfoInner::FindPeer(target) => {
                        let event = KademliaOut::FindNodeResult {
                            key: target,
                            closer_peers: closer_peers.collect(),
                        };
                        break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    QueryInfoInner::GetProviders { target, pending_results } => {
                        let event = KademliaOut::GetProvidersResult {
                            key: target,
                            closer_peers: closer_peers.collect(),
                            provider_peers: pending_results,
                        };

                        break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    },
                    QueryInfoInner::AddProvider { target } => {
                        for closest in closer_peers {
                            let event = NetworkBehaviourAction::SendEvent {
                                peer_id: closest,
                                event: KademliaHandlerIn::AddProvider {
                                    key: target.clone(),
                                    provider_peer: build_kad_peer(&KadHash::from(parameters.local_peer_id().clone()), &mut self.kbuckets),
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
}

/// Builds a `KadPeer` struct corresponding to the given `PeerId`.
/// The `PeerId` cannot be the same as the local one.
///
/// > **Note**: This is just a convenience function that doesn't do anything note-worthy.
fn build_kad_peer(
    kad_hash: &KadHash,
    kbuckets: &mut KBucketsTable<KadHash, Addresses>
) -> KadPeer {
    let (multiaddrs, connection_ty) = match kbuckets.entry(kad_hash) {
        kbucket::Entry::NotInKbucket(_) => (Vec::new(), KadConnectionType::NotConnected),       // TODO: pending connection?
        kbucket::Entry::InKbucketConnected(mut entry) => (entry.value().iter().cloned().collect(), KadConnectionType::Connected),
        kbucket::Entry::InKbucketDisconnected(mut entry) => (entry.value().iter().cloned().collect(), KadConnectionType::NotConnected),
        kbucket::Entry::InKbucketConnectedPending(mut entry) => (entry.value().iter().cloned().collect(), KadConnectionType::Connected),
        kbucket::Entry::InKbucketDisconnectedPending(mut entry) => (entry.value().iter().cloned().collect(), KadConnectionType::NotConnected),
        kbucket::Entry::SelfEntry => panic!("build_kad_peer expects not to be called with the KadHash of the local ID"),
    };

    KadPeer {
        node_id: kad_hash.peer_id().clone(),
        multiaddrs,
        connection_ty,
    }
}
