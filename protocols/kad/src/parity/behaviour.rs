// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::gen_random_id;
use crate::addresses::Addresses;
use crate::kbucket::KBucketsPeerId;
use crate::parity::{protocol, view::NodeRefMutViewState, view::View, Namespace};
use crate::query::{QueryConfig, QueryState, QueryStatePollOut};
use fnv::FnvHashMap;
use futures::prelude::*;
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::OneShotHandler, protocols_handler::ProtocolsHandler, Multiaddr, PeerId, upgrade};
use smallvec::SmallVec;
use std::{cmp, error, marker::PhantomData, time::Duration, time::Instant};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Interval;

mod test;

const RECORDS_REFRESH_INTERVAL: Duration = Duration::from_secs(3540);
const RECORDS_EXPIRE: Duration = Duration::from_secs(3600);

/// Network behaviour that handles Kademlia.
pub struct Kademlia<TSubstream> {
    /// How we view the network.
    view: View,

    /// For each key, list of nodes that provide this key, plus an expiration time for the entry.
    record_store: FnvHashMap<[u8; 32], SmallVec<[(protocol::KadRecord, Instant); 2]>>,

    /// Records that the local node is publishing. Contains an interval when to refresh the record
    /// so that it doesn't expire, and the value of the counter.
    ///
    /// Whenever we publish a record on the DHT, we include a linearly increasing number so that
    /// nodes that receive the record know which one is the most recent. This number is signed as
    /// part of the record. At initialization, we set this value to 0 but we also look for our own
    /// record in the DHT in order to know the value of this counter that may have been inserted
    /// by our node earlier in time.
    self_records: SmallVec<[(schnorrkel::Keypair, Interval, u64); 1]>,

    /// All the iterative queries we are currently performing, with their ID.
    active_queries: SmallVec<[QueryState<QueryInfo, (Namespace, PeerId)>; 8]>,

    /// List of queries to start once we are inside `poll()`.
    queries_to_starts: SmallVec<[QueryInfo; 8]>,

    /// Contains a list of peer IDs which we are not connected to, and an RPC query to send to them
    /// once they connect.
    pending_rpcs: SmallVec<[(PeerId, protocol::KadRequest); 8]>,

    /// Requests received by a remote that we should fulfill as soon as possible, with the peer
    /// that sent the request.
    remote_requests: SmallVec<[(PeerId, protocol::KadListenOut<TSubstream>); 4]>,

    /// Futures that write back our responses to the socket.
    // TODO:
    send_back_futures: SmallVec<[upgrade::WriteOne<TSubstream>; 12]>,

    /// `α` in the Kademlia reference papers. Designates the maximum number of queries that we
    /// perform in parallel.
    parallelism: usize,

    /// `k` in the Kademlia reference papers. Number of results in a find node query.
    num_results: usize,

    /// Timeout for each individual RPC query.
    rpc_timeout: Duration,

    /// Events to return when polling.
    queued_events: SmallVec<[NetworkBehaviourAction<protocol::KadRequest, KademliaOut>; 32]>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

/// Information about a peer.
#[derive(Debug, Clone)]
struct PeerInfo {
    /// List of addresses that we will send to remotes.
    addresses: Addresses,

    /// Namespace this node belongs to. A peer can only belong to one namespace, otherwise the
    /// Kademlia-with-prefix system doesn't work properly anymore.
    namespace: Namespace,
}

/// Local state of a query.
#[derive(Debug, Clone)]
struct QueryInfo {
    /// What we are querying.
    target: QueryTarget,
    /// Why we are querying.
    purpose: QueryPurpose,
    /// Temporary addresses used when trying to reach nodes.
    untrusted_addresses: FnvHashMap<PeerId, SmallVec<[Multiaddr; 8]>>,
}

impl KBucketsPeerId<(Namespace, PeerId)> for QueryInfo {
    fn distance_with(&self, other: &(Namespace, PeerId)) -> u32 {
        match self.target {
            QueryTarget::FindPeer(ref id) => id.distance_with(other),
            QueryTarget::FindProviders(ref key, _) => unimplemented!(),
        }
    }

    fn max_distance() -> usize {
        <(Namespace, PeerId) as KBucketsPeerId>::max_distance()
    }
}

impl PartialEq<(Namespace, PeerId)> for QueryInfo {
    fn eq(&self, other: &(Namespace, PeerId)) -> bool {
        match self.target {
            QueryTarget::FindPeer(ref id) => *id == *other,
            QueryTarget::FindProviders(ref key, _) => unimplemented!(),
        }
    }
}

/// Target of a query.
#[derive(Debug, Clone)]
enum QueryTarget {
    /// Finding a peer.
    FindPeer((Namespace, PeerId)),
    /// Find the peers that provide a certain value. Contains the resulting records that will be
    /// sent back.
    FindProviders([u8; 32], Vec<protocol::KadRecord>),
}

impl QueryTarget {
    /// Builds a protocol message corresponding to what we're looking for.
    fn to_rpc_request(&self) -> protocol::KadRequest {
        match self {
            QueryTarget::FindPeer((ref namespace, ref peer_id)) => protocol::KadRequest::FindPeer(protocol::KadPeerId {
                namespace: namespace.0,
                peer_id: peer_id.clone(),
            }),
            QueryTarget::FindProviders(ref key, _) => protocol::KadRequest::FindProviders(*key),
        }
    }
}

/// Reason why we have this query in the list of queries.
#[derive(Debug, Clone, PartialEq, Eq)]
enum QueryPurpose {
    /// The query was created for the Kademlia initialization process.
    Internal,
    /// The user requested this query to be performed. It should be reported when finished.
    UserRequest,
}

impl<TSubstream> Kademlia<TSubstream> {
    /// Creates a `Kademlia`.
    #[inline]
    pub fn new(local_namespace: Namespace, local_peer_id: PeerId) -> Self {
        Self::new_inner(local_namespace, local_peer_id, true)
    }

    /// Creates a `Kademlia`.
    ///
    /// Contrary to `new`, doesn't perform the initialization queries that store our local ID into
    /// the DHT.
    #[inline]
    pub fn without_init(local_namespace: Namespace, local_peer_id: PeerId) -> Self {
        Self::new_inner(local_namespace, local_peer_id, false)
    }

    /// Returns the namespace of the local node.
    pub fn local_namespace(&self) -> Namespace {
        self.view.my_id().0
    }

    /// Adds a known node to the local k-buckets. Doesn't do anything if the k-buckets are not
    /// capable of including a new node because they are full.
    // TODO: move bootstrap nodes in the config?
    pub fn add_bootstrap_node(&mut self, namespace: Namespace, peer_id: PeerId, addr: Multiaddr) {
        let mut in_view = match self.view.node_mut(&peer_id).into_view_state() {
            NodeRefMutViewState::InView(in_view) => in_view,
            NodeRefMutViewState::NotInView(not_in_view) => {
                match not_in_view.set_namespace(namespace) {
                    NodeRefMutViewState::InView(in_view) => in_view,
                    NodeRefMutViewState::NotInView(_) => return,
                }
            }
        };

        in_view.add_address(&addr);
    }

    /// Broadcasts to the DHT that we are providing the given key.
    ///
    /// Has no effect if `add_providing_key` has been called with that same key earlier.
    pub fn add_providing_key(&mut self, key: schnorrkel::Keypair) {
        // Check for duplicates.
        if self.self_records.iter().any(|(k, _, _)| k.public == key.public) {
            return;
        }

        // We start a query in order to retreive any possible value already in the DHT, so that we
        // get the value of the `counter`.
        self.start_query(QueryTarget::FindProviders(key.public.to_bytes(), Vec::new()), QueryPurpose::Internal);
        self.self_records.push((key, Interval::new(Instant::now(), RECORDS_REFRESH_INTERVAL), 0));
    }

    /// If we were providing the given key, removes it from the list.
    ///
    /// This will not remove the key from the DHT, but will stop the periodic refresh.
    pub fn remove_providing_key(&mut self, key: &schnorrkel::PublicKey) {
        self.self_records.retain(|(k, _, _)| k.public != *key);
        self.self_records.shrink_to_fit();
    }

    /// Stores addresses in Kademlia. The addresses are deemed trusted, and can be sent to other
    /// peers.
    ///
    /// Has no effect if the peer is not in our k-buckets.
    #[inline]
    pub fn add_reportable_addresses<'a>(&mut self, peer_id: &PeerId, addrs: impl IntoIterator<Item = &'a Multiaddr>) {
        if let Some(mut in_view) = self.view.node_mut(peer_id).into_in_view() {
            in_view.add_addresses(addrs)
        }
    }

    /// Starts an iterative `FIND_NODE` request.
    ///
    /// This will eventually produce an event containing the nodes of the DHT closest to the
    /// requested `PeerId`.
    #[inline]
    pub fn find_node(&mut self, namespace: Namespace, peer_id: PeerId) {
        self.start_query(QueryTarget::FindPeer((namespace, peer_id)), QueryPurpose::UserRequest);
    }

    /// Starts an iterative request to find the list of nodes that are able to sign with the given
    /// key.
    ///
    /// This will eventually produce an event containing the nodes of the DHT that are providing
    /// this key.
    #[inline]
    pub fn find_providers(&mut self, key: [u8; 32]) {
        self.start_query(QueryTarget::FindProviders(key, Vec::new()), QueryPurpose::UserRequest);
    }

    /// Inner implementation of the constructors.
    fn new_inner(local_namespace: Namespace, local_peer_id: PeerId, initialize: bool) -> Self {
        let my_id = protocol::KadPeerId { namespace: local_namespace.0, peer_id: local_peer_id.clone() };

        let mut behaviour = Kademlia {
            view: View::new(local_namespace, local_peer_id),
            record_store: FnvHashMap::default(),
            self_records: SmallVec::new(),
            queued_events: SmallVec::new(),
            queries_to_starts: SmallVec::new(),
            send_back_futures: SmallVec::new(),
            active_queries: Default::default(),
            pending_rpcs: SmallVec::with_capacity(3),
            remote_requests: SmallVec::new(),
            parallelism: 3,
            num_results: 20,
            rpc_timeout: Duration::from_secs(8),
            marker: PhantomData,
        };

        if initialize {
            // As part of the initialization process, we start one `FIND_NODE` for each bit of the
            // possible range of peer IDs.
            for n in 0..256 {
                let peer_id = match gen_random_id(&behaviour.view.my_id().1, n) {
                    Ok(p) => p,
                    Err(()) => continue,
                };

                let with_ns = (behaviour.view.my_id().0, peer_id);
                behaviour.start_query(QueryTarget::FindPeer(with_ns), QueryPurpose::Internal);
            }
        }

        behaviour
    }

    /// Builds a `KadPeer` struct corresponding to the given `PeerId`.
    /// The `PeerId` can be the same as the local one.
    ///
    /// > **Note**: This is just a convenience function that doesn't do anything note-worthy.
    fn build_kad_peer(
        &mut self,
        id: (Namespace, PeerId),
        parameters: &mut PollParameters,
    ) -> protocol::KadPeer {
        let addresses = if id.1 == *parameters.local_peer_id() {
            let mut addrs = parameters
                .listened_addresses()
                .cloned()
                .collect::<Vec<_>>();
            addrs.extend(parameters.external_addresses());
            addrs
        } else if let Some(in_view) = self.view.node_mut(&id.1).into_in_view() {
            in_view.addresses().cloned().collect()
        } else {
            Vec::new()
        };

        protocol::KadPeer {
            id: protocol::KadPeerId {
                namespace: (id.0).0,
                peer_id: id.1,
            },
            addresses,
        }
    }

    /// Processes a response from the server to one of our requests.
    fn process_response(&mut self, source: PeerId, original_request: protocol::KadRequest, response: Option<protocol::KadResponse>) {
        match (original_request, response) {
            (protocol::KadRequest::FindPeer(peer_id), Some(protocol::KadResponse::FindPeer { closer_peers })) => {
                for query in self.active_queries.iter_mut() {
                    match query.target().target {
                        QueryTarget::FindPeer(ref p) if (p.0).0 == peer_id.namespace && p.1 == peer_id.peer_id => (),
                        _ => continue
                    };

                    let id = if let Some(id) = query.waiting().find(|p| p.1 == source) {
                        id.clone()
                    } else {
                        continue
                    };

                    query.inject_rpc_result(&id, closer_peers.clone().into_iter()
                        .map(|p| (Namespace(p.id.namespace), p.id.peer_id)));

                    for peer in &closer_peers {
                        self.queued_events.push(NetworkBehaviourAction::GenerateEvent(KademliaOut::Discovered {
                            source: source.clone(),
                            discovered: (Namespace(peer.id.namespace), peer.id.peer_id.clone()),
                            addresses: peer.addresses.clone(),
                        }));

                        let addrs = query.target_mut()
                            .untrusted_addresses
                            .entry(peer.id.peer_id.clone())
                            .or_default();
                        for addr in &peer.addresses {
                            addrs.push(addr.clone());
                        }
                    }
                }
            },
            (protocol::KadRequest::FindProviders(key), Some(protocol::KadResponse::FindProviders { closer_peers, mut records })) => {
                let schnorrkey = schnorrkel::PublicKey::from_bytes(&key[..])
                    .expect("original_request is a copy of a message we sent, and we always send \
                             messages with a valid key");

                // If we searched for one of our own records (which we do at initialization), check
                // the value of the counter.
                if let Some((_, _, counter)) = self.self_records.iter_mut().find(|(record, _, _)| record.public == schnorrkey) {
                    if let Some(rx_counter) = records.iter().map(|rec| rec.counter).max() {
                        *counter = cmp::max(*counter, rx_counter.saturating_add(1));
                    }
                }

                // Add the produced records to the relevant active queries.
                for query in self.active_queries.iter_mut() {
                    if let QueryTarget::FindProviders(ref qk, ref mut out) = query.target_mut().target {
                        if *qk == key {
                            // TODO: is draining correct?
                            for record in records.drain(..) {
                                if let Ok(record) = record.verify(&schnorrkey) {
                                    // TODO: add addresses to list
                                    out.push(record);
                                } else {
                                    // TODO: log and report this
                                }
                            }

                            // TODO: inject_rpc_result in the query
                        }
                    }
                }
            },
            (protocol::KadRequest::AddProvider(_, _), None) => {},
            (protocol::KadRequest::NamespaceReport(_), None) => {},
            (rq, rp) => {
                // Bad combination of request and response.
                panic!("{:?} {:?}", rq, rp) // TODO: log and report this
            }
        }
    }

    /// Processes a request, and sends back the answer to it.
    fn process_request(&mut self, source: PeerId, request: protocol::KadListenOut<TSubstream>, parameters: &mut PollParameters)
    where
        TSubstream: AsyncWrite,
    {
        match request.request() {
            protocol::KadRequest::NamespaceReport(namespace) => {
                if let Some(mut not_in_view) = self.view.node_mut(&source).into_not_in_view() {
                    not_in_view.set_namespace(Namespace::from(*namespace));
                }
            }

            protocol::KadRequest::FindPeer(ref target) => {
                let mut closer_peers = Vec::with_capacity(self.num_results);
                for peer in self.view.find_closest(target).take(self.num_results) {
                    let kad_peer = self.build_kad_peer(peer, parameters);
                    closer_peers.push(kad_peer);
                }
                println!("received find peer for {:?}; sending back {:?}", target, closer_peers);
                self.send_back_futures.push(request.respond(protocol::KadResponse::FindPeer { closer_peers }));
            }

            protocol::KadRequest::AddProvider(key, record) => {
                if let Ok(pubkey) = schnorrkel::PublicKey::from_bytes(&key[..]) {
                    // TODO: borrow error
                    /*if let Ok(record) = record.verify(&pubkey) {
                        let expiration = Instant::now() + RECORDS_EXPIRE;
                        let mut entries = self.record_store.entry(*key).or_default();
                        if let Some((old_rec, old_exp)) = entries.iter_mut().find(|(r, _)| record.should_replace(r)) {
                            *old_rec = record;
                            *old_exp = expiration;
                        } else {
                            entries.push((record, expiration));
                        }
                    }*/
                }
            }

            protocol::KadRequest::FindProviders(key) => {
                let now = Instant::now();
                let records = self.record_store
                    .get(key)
                    .into_iter()
                    .flat_map(|list| list.iter())
                    .filter(|(_, exp)| *exp > now)
                    .map(|(record, _)| protocol::UnverifiedKadRecord::from(record.clone()))
                    .collect();

                let mut closer_peers = Vec::with_capacity(self.num_results);
                // TODO: wrong
                /*for peer in self.kbuckets.find_closest_with_self(key).take(self.num_results) {
                    let kad_peer = self.build_kad_peer(peer, parameters);
                    closer_peers.push(kad_peer);
                }*/

                self.send_back_futures.push(request.respond(protocol::KadResponse::FindProviders {
                    closer_peers,
                    records,
                }));
            }
        }
    }

    /// Internal function that starts a query.
    fn start_query(&mut self, target: QueryTarget, purpose: QueryPurpose) {
        self.queries_to_starts.push(QueryInfo {
            target,
            purpose,
            untrusted_addresses: Default::default(),
        });
    }
}

impl<TSubstream> NetworkBehaviour for Kademlia<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = OneShotHandler<TSubstream, protocol::KadListen, protocol::KadRequest, InnerMessage<TSubstream>>;
    type OutEvent = KademliaOut;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        OneShotHandler::default()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        // Start with the trusted addresses from the view.
        let mut addrs = if let Some(node) = self.view.node_mut(peer_id).into_in_view() {
            node.addresses().cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        // Add the addresses from the records we store.
        for peer in self.record_store.values().flat_map(|l| l.iter()).map(|r| r.0.identity()) {
            if peer.id.peer_id == *peer_id {
                addrs.extend(peer.addresses.iter().cloned());
            }
        }

        // Add temporary addresses for active queries.
        // When we want to reach a node whose addresses are only known through a previous query,
        // we need to add them.
        // This is necessary for the iterative querying system to work.
        // Note that this block is at the very bottom, because the addresses should have the lowest
        // piority.
        for query in &self.active_queries {
            if let Some(a) = query.target().untrusted_addresses.get(peer_id) {
                addrs.extend(a.iter().cloned());
            }
        }

        addrs
    }

    fn inject_connected(&mut self, id: PeerId, endpoint: ConnectedPoint) {
        // Update the view.
        self.view.node_mut(&id)
            .into_disconnected()
            .expect("The view is kept in sync with the swarm; QED")
            .set_connected(endpoint);

        // Ask the node for its namespace.
        self.queued_events.push(NetworkBehaviourAction::SendEvent {
            peer_id: id.clone(),
            event: protocol::KadRequest::NamespaceReport((self.view.my_id().0).0),
        });

        // Dispatch pending RPC requests.
        if let Some(pos) = self.pending_rpcs.iter().position(|(p, _)| p == &id) {
            let (peer_id, rpc) = self.pending_rpcs.remove(pos);
            self.queued_events.push(NetworkBehaviourAction::SendEvent {
                peer_id,
                event: rpc,
            });
        }
    }

    fn inject_dial_failure(&mut self, peer_id: Option<&PeerId>, addr: &Multiaddr, _: &dyn error::Error) {
        if let Some(peer_id) = peer_id {
            if let Some(mut node) = self.view.node_mut(peer_id).into_in_view() {
                node.remove_address(addr);
            }
        }
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId, _: ConnectedPoint) {
        self.view.node_mut(peer_id)
            .into_connected()
            .expect("The view is kept in sync with the swarm; QED")
            .set_disconnected();

        for query in self.active_queries.iter_mut() {
            // Note: for some reason, combining the two lines below into one fails to compile.
            let waiting = query.waiting().find(|id| id.1 == *peer_id).map(|i| i.clone());
            if let Some(id) = waiting {
                query.inject_rpc_error(&id);
            }
        }
    }

    fn inject_replaced(&mut self, peer_id: PeerId, old_endpoint: ConnectedPoint, new_endpoint: ConnectedPoint) {
        // Update the view.
        self.view.node_mut(&peer_id)
            .into_connected()
            .expect("The view is kept in sync with the swarm; QED")
            .replace_connected_point(new_endpoint);

        // We need to re-send the active queries.
        for query in self.active_queries.iter() {
            if query.waiting().any(|id| id.1 == peer_id) {
                self.queued_events.push(NetworkBehaviourAction::SendEvent {
                    peer_id: peer_id.clone(),
                    event: query.target().target.to_rpc_request(),
                });
            }
        }

        // If we don't know the namespace yet, we resend a namespace query because the previous
        // one may have been ignored.
        if let Some(_) = self.view.node_mut(&peer_id).into_not_in_view() {
            self.queued_events.push(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: protocol::KadRequest::NamespaceReport((self.view.my_id().0).0),
            });
        }
    }

    fn inject_node_event(&mut self, source: PeerId, event: InnerMessage<TSubstream>) {
        match event {
            InnerMessage::Rx(request) => {
                self.remote_requests.push((source, request));
            },
            InnerMessage::Tx(request, response) => {
                self.process_response(source, request, response);
            },
        }
    }

    fn poll(
        &mut self,
        parameters: &mut PollParameters,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Clearing all expired records.
        // TODO: move lower in the code
        let now = Instant::now();
        self.record_store.retain(|_, list| {
            list.retain(|(_, exp)| *exp > now);
            !list.is_empty()
        });

        // Start queries that are waiting to start.
        for query_info in self.queries_to_starts.drain() {
            let known_closest_peers = self.view
                .find_closest(&query_info)
                .take(self.num_results);
            self.active_queries.push(QueryState::new(QueryConfig {
                target: query_info,
                parallelism: self.parallelism,
                num_results: self.num_results,
                rpc_timeout: self.rpc_timeout,
                known_closest_peers,
            }));
        }
        self.queries_to_starts.shrink_to_fit();

        // Handle remote queries.
        if !self.remote_requests.is_empty() {
            let (source, received_request) = self.remote_requests.remove(0);
            self.process_request(source, received_request, parameters);
        }

        // Removes each future one by one, and pushes them back if they're not ready.
        for n in (0..self.send_back_futures.len()).rev() {
            let mut future = self.send_back_futures.swap_remove(n);
            match future.poll() {
                Ok(Async::Ready(())) => {},
                Ok(Async::NotReady) => self.send_back_futures.push(future),
                Err(err) => { panic!("{:?}", err) },     // TODO: report?
            }
        }

        loop {
            // Handle events queued by other parts of this struct
            if !self.queued_events.is_empty() {
                return Async::Ready(self.queued_events.remove(0));
            }
            self.queued_events.shrink_to_fit();

            // If iterating finds a query that is finished, stores here its position in the list
            // and stops looping.
            let mut finished_query = None;

            'queries_iter: for (pos, query) in self.active_queries.iter_mut().enumerate() {
                loop {
                    match query.poll() {
                        Async::Ready(QueryStatePollOut::Finished) => {
                            finished_query = Some(pos);
                            break 'queries_iter;
                        }
                        Async::Ready(QueryStatePollOut::SendRpc {
                            peer_id,
                            query_target,
                        }) => {
                            let rpc = query_target.target.to_rpc_request();
                            if let Some(_) = self.view.node_mut(&peer_id.1).into_connected() {
                                return Async::Ready(NetworkBehaviourAction::SendEvent {
                                    peer_id: peer_id.1.clone(),
                                    event: rpc,
                                });
                            } else {
                                self.pending_rpcs.push((peer_id.1.clone(), rpc));
                                return Async::Ready(NetworkBehaviourAction::DialPeer {
                                    peer_id: peer_id.1.clone(),
                                });
                            }
                        }
                        Async::Ready(QueryStatePollOut::CancelRpc { peer_id }) => {
                            // We don't cancel if the RPC has already been sent out.
                            self.pending_rpcs.retain(|(id, _)| *id != peer_id.1);
                        }
                        Async::NotReady => break,
                    }
                }
            }

            if let Some(finished_query) = finished_query {
                let (target, closer_peers) = self.active_queries.remove(finished_query).into_target_and_closest_peers();
                if let QueryPurpose::UserRequest = target.purpose {
                    let event = match target.target {
                        QueryTarget::FindPeer(key) => {
                            KademliaOut::FindNodeResult { key, closer_peers: closer_peers.collect() }
                        },
                        QueryTarget::FindProviders(key, provider_peers) => {
                            KademliaOut::FindProvidersResult {
                                key,
                                provider_peers: provider_peers
                                    .into_iter()
                                    .map(|r| r.into_identity())
                                    .map(|k| (Namespace(k.id.namespace), k.id.peer_id, k.addresses.into_iter().collect()))
                                    .collect(),
                            }
                        },
                    };

                    break Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                }
            } else {
                break Async::NotReady;
            }
        }
    }
}

/// Transmission between the `OneShotHandler` and the `Kademlia`.
#[doc(hidden)]
pub enum InnerMessage<TSubstream> {
    /// We received an RPC request from a remote.
    Rx(protocol::KadListenOut<TSubstream>),
    /// We successfully sent an RPC request.
    Tx(protocol::KadRequest, Option<protocol::KadResponse>),
}

impl<TSubstream> From<protocol::KadListenOut<TSubstream>> for InnerMessage<TSubstream> {
    #[inline]
    fn from(rq: protocol::KadListenOut<TSubstream>) -> Self {
        InnerMessage::Rx(rq)
    }
}

impl<TSubstream> From<(protocol::KadRequest, Option<protocol::KadResponse>)> for InnerMessage<TSubstream> {
    #[inline]
    fn from((rq, rp): (protocol::KadRequest, Option<protocol::KadResponse>)) -> Self {
        InnerMessage::Tx(rq, rp)
    }
}

/// Output event of the `Kademlia` behaviour.
#[derive(Debug, Clone)]
pub enum KademliaOut {
    /// We inserted a node in our k-buckets. This means that this node will be included in results
    /// to RPC queries.
    ///
    /// After this event happened, you are encouraged to query the node for the list of addresses
    /// it is listening on (through a mean not covered by Kademlia), and call
    /// `add_reportable_addresses`.
    KbucketsInsertion {
        /// Id of the node that has been inserted.
        peer_id: (Namespace, PeerId),
    },

    /// We removed a node from our k-buckets. It will no longer be included in results to RPC
    /// queries.
    KbucketsRemoval {
        /// Id of the node that has been removed.
        peer_id: (Namespace, PeerId),
    },

    /// We have heard about a node from a remote.
    ///
    /// One or more nodes we are connected to sent us the identity of this node.
    ///
    /// Kademlia doesn't keep track of discovered nodes, so this event will likely be called
    /// multiple times with the same node. This event will also be generated for nodes in the
    /// k-buckets.
    // TODO: can this include the local peer?
    Discovered {
        /// Node that provided the identity of the discovered node.
        source: PeerId,
        /// Id of the node that has been discovered.
        discovered: (Namespace, PeerId),
        /// Addresses of the node that has been discovered.
        addresses: Vec<Multiaddr>,
    },

    /// Result of a `FIND_NODE` iterative query.
    FindNodeResult {
        /// The key that we looked for in the query.
        key: (Namespace, PeerId),

        /// List of peers that are closed to the key, ordered from closest to furthest away.
        closer_peers: Vec<(Namespace, PeerId)>,
    },

    /// Result of finding a provider.
    FindProvidersResult {
        /// The key that we were looking for.
        key: [u8; 32],

        /// The peers that are providing the requested key.
        ///
        /// > **Note**: We are guaranteed that these peers actually own the key, as they have
        /// >           provided a valid signature containing their identity.
        ///
        /// > **Note**: The implementation of `addresses_of_peer` of `Kademlia` will **not**
        /// >           necessarily include the addresses returned here. In other words, if you're
        /// >           using the `Swarm`, you have to store these addresses somewhere and have
        /// >           an implementation of `addresses_of_peer` that stores them.
        // TODO: store the addresses anyway?
        // TODO: use a better type
        provider_peers: Vec<(Namespace, PeerId, SmallVec<[Multiaddr; 4]>)>,
    },
}
