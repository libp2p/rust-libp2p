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

//! This module handles performing iterative queries about the network.

use fnv::FnvHashSet;
use futures::{future, Future, stream, Stream};
use kbucket::KBucketsPeerId;
use libp2p_core::PeerId;
use multiaddr::{Protocol, Multiaddr};
use protocol;
use rand;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::io::Error as IoError;
use std::mem;

/// Parameters of a query. Allows plugging the query-related code with the rest of the
/// infrastructure.
pub struct QueryParams<FBuckets, FFindNode> {
    /// Identifier of the local peer.
    pub local_id: PeerId,
    /// Called whenever we need to obtain the peers closest to a certain peer.
    pub kbuckets_find_closest: FBuckets,
    /// Level of parallelism for networking. If this is `N`, then we can dial `N` nodes at a time.
    pub parallelism: usize,
    /// Called whenever we want to send a `FIND_NODE` RPC query.
    pub find_node: FFindNode,
}

/// Event that happens during a query.
#[derive(Debug, Clone)]
pub enum QueryEvent<TOut> {
    /// Learned about new mutiaddresses for the given peers.
    PeersReported(Vec<(PeerId, Vec<Multiaddr>)>),
    /// Finished the processing of the query. Contains the result.
    Finished(TOut),
}

/// Starts a query for an iterative `FIND_NODE` request.
#[inline]
pub fn find_node<'a, FBuckets, FFindNode>(
    query_params: QueryParams<FBuckets, FFindNode>,
    searched_key: PeerId,
) -> Box<Stream<Item = QueryEvent<Vec<PeerId>>, Error = IoError> + Send + 'a>
where
    FBuckets: Fn(PeerId) -> Vec<PeerId> + 'a + Clone,
    FFindNode: Fn(Multiaddr, PeerId) -> Box<Future<Item = Vec<protocol::Peer>, Error = IoError> + Send> + 'a + Clone,
{
    query(query_params, searched_key, 20) // TODO: constant
}

/// Refreshes a specific bucket by performing an iterative `FIND_NODE` on a random ID of this
/// bucket.
///
/// Returns a dummy no-op future if `bucket_num` is out of range.
pub fn refresh<'a, FBuckets, FFindNode>(
    query_params: QueryParams<FBuckets, FFindNode>,
    bucket_num: usize,
) -> Box<Stream<Item = QueryEvent<()>, Error = IoError> + Send + 'a>
where
    FBuckets: Fn(PeerId) -> Vec<PeerId> + 'a + Clone,
    FFindNode: Fn(Multiaddr, PeerId) -> Box<Future<Item = Vec<protocol::Peer>, Error = IoError> + Send> + 'a + Clone,
{
    let peer_id = match gen_random_id(&query_params.local_id, bucket_num) {
        Ok(p) => p,
        Err(()) => return Box::new(stream::once(Ok(QueryEvent::Finished(())))),
    };

    let stream = find_node(query_params, peer_id).map(|event| {
        match event {
            QueryEvent::PeersReported(peers) => QueryEvent::PeersReported(peers),
            QueryEvent::Finished(_) => QueryEvent::Finished(()),
        }
    });

    Box::new(stream) as Box<_>
}

// Generates a random `PeerId` that belongs to the given bucket.
//
// Returns an error if `bucket_num` is out of range.
fn gen_random_id(my_id: &PeerId, bucket_num: usize) -> Result<PeerId, ()> {
    let my_id_len = my_id.as_bytes().len();

    // TODO: this 2 is magic here ; it is the length of the hash of the multihash
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

// Generic query-performing function.
fn query<'a, FBuckets, FFindNode>(
    query_params: QueryParams<FBuckets, FFindNode>,
    searched_key: PeerId,
    num_results: usize,
) -> Box<Stream<Item = QueryEvent<Vec<PeerId>>, Error = IoError> + Send + 'a>
where
    FBuckets: Fn(PeerId) -> Vec<PeerId> + 'a + Clone,
    FFindNode: Fn(Multiaddr, PeerId) -> Box<Future<Item = Vec<protocol::Peer>, Error = IoError> + Send> + 'a + Clone,
{
    debug!("Start query for {:?} ; num results = {}", searched_key, num_results);

    // State of the current iterative process.
    struct State<'a> {
        // At which stage we are.
        stage: Stage,
        // Final output of the iteration.
        result: Vec<PeerId>,
        // For each open connection, a future with the response of the remote.
        // Note that don't use a `SmallVec` here because `select_all` produces a `Vec`.
        current_attempts_fut: Vec<Box<Future<Item = Vec<protocol::Peer>, Error = IoError> + Send + 'a>>,
        // For each open connection, the peer ID that we are connected to.
        // Must always have the same length as `current_attempts_fut`.
        current_attempts_addrs: SmallVec<[PeerId; 32]>,
        // Nodes that need to be attempted.
        pending_nodes: Vec<PeerId>,
        // Peers that we tried to contact but failed.
        failed_to_contact: FnvHashSet<PeerId>,
    }

    // General stage of the state.
    #[derive(Copy, Clone, PartialEq, Eq)]
    enum Stage {
        // We are still in the first step of the algorithm where we try to find the closest node.
        FirstStep,
        // We are contacting the k closest nodes in order to fill the list with enough results.
        SecondStep,
        // The results are complete, and the next stream iteration will produce the outcome.
        FinishingNextIter,
        // We are finished and the stream shouldn't return anything anymore.
        Finished,
    }

    let initial_state = State {
        stage: Stage::FirstStep,
        result: Vec::with_capacity(num_results),
        current_attempts_fut: Vec::new(),
        current_attempts_addrs: SmallVec::new(),
        pending_nodes: {
            let kbuckets_find_closest = query_params.kbuckets_find_closest.clone();
            kbuckets_find_closest(searched_key.clone())     // TODO: suboptimal
        },
        failed_to_contact: Default::default(),
    };

    let parallelism = query_params.parallelism;

    // Start of the iterative process.
    let stream = stream::unfold(initial_state, move |mut state| -> Option<_> {
        match state.stage {
            Stage::FinishingNextIter => {
                let result = mem::replace(&mut state.result, Vec::new());
                debug!("Query finished with {} results", result.len());
                state.stage = Stage::Finished;
                let future = future::ok((Some(QueryEvent::Finished(result)), state));
                return Some(future::Either::A(future));
            },
            Stage::Finished => {
                return None;
            },
            _ => ()
        };

        let searched_key = searched_key.clone();
        let find_node_rpc = query_params.find_node.clone();

        // Find out which nodes to contact at this iteration.
        let to_contact = {
            let wanted_len = if state.stage == Stage::FirstStep {
                parallelism.saturating_sub(state.current_attempts_fut.len())
            } else {
                num_results.saturating_sub(state.current_attempts_fut.len())
            };
            let mut to_contact = SmallVec::<[_; 16]>::new();
            while to_contact.len() < wanted_len && !state.pending_nodes.is_empty() {
                // Move the first element of `pending_nodes` to `to_contact`, but ignore nodes that
                // are already part of the results or of a current attempt or if we failed to
                // contact it before.
                let peer = state.pending_nodes.remove(0);
                if state.result.iter().any(|p| p == &peer) {
                    continue;
                }
                if state.current_attempts_addrs.iter().any(|p| p == &peer) {
                    continue;
                }
                if state.failed_to_contact.iter().any(|p| p == &peer) {
                    continue;
                }
                to_contact.push(peer);
            }
            to_contact
        };

        debug!("New query round ; {} queries in progress ; contacting {} new peers",
               state.current_attempts_fut.len(),
               to_contact.len());

        // For each node in `to_contact`, start an RPC query and a corresponding entry in the two
        // `state.current_attempts_*` fields.
        for peer in to_contact {
            let multiaddr: Multiaddr = Protocol::P2p(peer.clone().into_bytes()).into();

            let searched_key2 = searched_key.clone();
            let current_attempt = find_node_rpc(multiaddr.clone(), searched_key2); // TODO: suboptimal
            state.current_attempts_addrs.push(peer.clone());
            state
                .current_attempts_fut
                .push(Box::new(current_attempt) as Box<_>);
        }
        debug_assert_eq!(
            state.current_attempts_addrs.len(),
            state.current_attempts_fut.len()
        );

        // Extract `current_attempts_fut` so that we can pass it to `select_all`. We will push the
        // values back when inside the loop.
        let current_attempts_fut = mem::replace(&mut state.current_attempts_fut, Vec::new());
        if current_attempts_fut.is_empty() {
            // If `current_attempts_fut` is empty, then `select_all` would panic. It happens
            // when we have no additional node to query.
            debug!("Finishing query early because no additional node available");
            state.stage = Stage::FinishingNextIter;
            let future = future::ok((None, state));
            return Some(future::Either::A(future));
        }

        // This is the future that continues or breaks the `loop_fn`.
        let future = future::select_all(current_attempts_fut.into_iter()).then(move |result| {
            let (message, trigger_idx, other_current_attempts) = match result {
                Err((err, trigger_idx, other_current_attempts)) => {
                    (Err(err), trigger_idx, other_current_attempts)
                }
                Ok((message, trigger_idx, other_current_attempts)) => {
                    (Ok(message), trigger_idx, other_current_attempts)
                }
            };

            // Putting back the extracted elements in `state`.
            let remote_id = state.current_attempts_addrs.remove(trigger_idx);
            debug_assert!(state.current_attempts_fut.is_empty());
            state.current_attempts_fut = other_current_attempts;

            // `message` contains the reason why the current future was woken up.
            let closer_peers = match message {
                Ok(msg) => msg,
                Err(err) => {
                    trace!("RPC query failed for {:?}: {:?}", remote_id, err);
                    state.failed_to_contact.insert(remote_id);
                    return future::ok((None, state));
                }
            };

            // Inserting the node we received a response from into `state.result`.
            // The code is non-trivial because `state.result` is ordered by distance and is limited
            // by `num_results` elements.
            if let Some(insert_pos) = state.result.iter().position(|e| {
                e.distance_with(&searched_key) >= remote_id.distance_with(&searched_key)
            }) {
                if state.result[insert_pos] != remote_id {
                    if state.result.len() >= num_results {
                        state.result.pop();
                    }
                    state.result.insert(insert_pos, remote_id);
                }
            } else if state.result.len() < num_results {
                state.result.push(remote_id);
            }

            // The loop below will set this variable to `true` if we find a new element to put at
            // the top of the result. This would mean that we have to continue looping.
            let mut local_nearest_node_updated = false;

            // Update `state` with the actual content of the message.
            let mut new_known_multiaddrs = Vec::with_capacity(closer_peers.len());
            for mut peer in closer_peers {
                // Update the peerstore with the information sent by
                // the remote.
                {
                    let multiaddrs = mem::replace(&mut peer.multiaddrs, Vec::new());
                    trace!("Reporting multiaddresses for {:?}: {:?}", peer.node_id, multiaddrs);
                    new_known_multiaddrs.push((peer.node_id.clone(), multiaddrs));
                }

                if peer.node_id.distance_with(&searched_key)
                    <= state.result[0].distance_with(&searched_key)
                {
                    local_nearest_node_updated = true;
                }

                if state.result.iter().any(|ma| ma == &peer.node_id) {
                    continue;
                }

                // Insert the node into `pending_nodes` at the right position, or do not
                // insert it if it is already in there.
                if let Some(insert_pos) = state.pending_nodes.iter().position(|e| {
                    e.distance_with(&searched_key) >= peer.node_id.distance_with(&searched_key)
                }) {
                    if state.pending_nodes[insert_pos] != peer.node_id {
                        state.pending_nodes.insert(insert_pos, peer.node_id.clone());
                    }
                } else {
                    state.pending_nodes.push(peer.node_id.clone());
                }
            }

            if state.result.len() >= num_results
                || (state.stage != Stage::FirstStep && state.current_attempts_fut.is_empty())
            {
                state.stage = Stage::FinishingNextIter;

            } else {
                if !local_nearest_node_updated {
                    trace!("Loop didn't update closer node ; jumping to step 2");
                    state.stage = Stage::SecondStep;
                }
            }

            future::ok((Some(QueryEvent::PeersReported(new_known_multiaddrs)), state))
        });

        Some(future::Either::B(future))
    }).filter_map(|val| val);

    Box::new(stream) as Box<_>
}
