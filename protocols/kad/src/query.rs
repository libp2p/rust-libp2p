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

//! Contains the iterative querying process of Kademlia.
//!
//! This allows one to create queries that iterate on the DHT on nodes that become closer and
//! closer to the target.

use crate::kbucket::KBucketsPeerId;
use futures::prelude::*;
use smallvec::SmallVec;
use std::{cmp::PartialEq, time::Duration, time::Instant};
use tokio_timer::Delay;

/// State of a query iterative process.
///
/// The API of this state machine is similar to the one of `Future`, `Stream` or `Swarm`. You need
/// to call `poll()` to query the state for actions to perform. If `NotReady` is returned, the
/// current task will be woken up automatically when `poll()` needs to be called again.
///
/// Note that this struct only handles iterating over nodes that are close to the target. For
/// `FIND_NODE` queries you don't need more than that. However for `FIND_VALUE` and
/// `GET_PROVIDERS`, you need to extract yourself the value or list of providers from RPC requests
/// received by remotes as this is not handled by the `QueryState`.
#[derive(Debug)]
pub struct QueryState<TTarget, TPeerId> {
    /// Target we're looking for.
    target: TTarget,

    /// Stage of the query. See the documentation of `QueryStage`.
    stage: QueryStage,

    /// Ordered list of the peers closest to the result we're looking for.
    /// Entries that are `InProgress` shouldn't be removed from the list before they complete.
    /// Must never contain two entries with the same peer IDs.
    closest_peers: SmallVec<[(TPeerId, QueryPeerState); 32]>,

    /// Allowed level of parallelism.
    parallelism: usize,

    /// Number of results to produce.
    num_results: usize,

    /// Timeout for each individual RPC query.
    rpc_timeout: Duration,
}

/// Configuration for a query.
#[derive(Debug, Clone)]
pub struct QueryConfig<TIter, TTarget> {
    /// Target of the query.
    pub target: TTarget,

    /// Iterator to a list of `num_results` nodes that we know of whose distance is close to the
    /// target.
    pub known_closest_peers: TIter,

    /// Allowed level of parallelism.
    pub parallelism: usize,

    /// Number of results to produce.
    pub num_results: usize,

    /// Timeout for each individual RPC query.
    pub rpc_timeout: Duration,
}

/// Stage of the query.
#[derive(Debug)]
enum QueryStage {
    /// We are trying to find a closest node.
    Iterating {
        /// Number of successful query results in a row that didn't find any closer node.
        // TODO: this is not great, because we don't necessarily receive responses in the order
        //       we made the queries. It is possible that we query multiple far-away nodes in a
        //       row, and obtain results before the result of the closest nodes.
        no_closer_in_a_row: usize,
    },

    // We have found the closest node, and we are now pinging the nodes we know about.
    Frozen,
}

impl<TTarget, TPeerId> QueryState<TTarget, TPeerId>
where
    TPeerId: Eq,
    TTarget: KBucketsPeerId<TPeerId>
{
    /// Creates a new query.
    ///
    /// You should call `poll()` this function returns in order to know what to do.
    pub fn new(config: QueryConfig<impl IntoIterator<Item = TPeerId>, TTarget>) -> Self {
        let mut closest_peers: SmallVec<[_; 32]> = config
            .known_closest_peers
            .into_iter()
            .map(|peer_id| (peer_id, QueryPeerState::NotContacted))
            .take(config.num_results)
            .collect();
        let target = config.target;
        closest_peers.sort_by_key(|e| target.distance_with(&e.0));
        closest_peers.dedup_by(|a, b| a.0 == b.0);

        QueryState {
            target,
            stage: QueryStage::Iterating {
                no_closer_in_a_row: 0,
            },
            closest_peers,
            parallelism: config.parallelism,
            num_results: config.num_results,
            rpc_timeout: config.rpc_timeout,
        }
    }

    /// Returns the target of the query. Always the same as what was passed to `new()`.
    #[inline]
    pub fn target(&self) -> &TTarget {
        &self.target
    }

    /// Returns the target of the query. Always the same as what was passed to `new()`.
    ///
    /// You shouldn't modify the target in such a way that modifies the target of the query,
    /// otherwise logic errors will likely happen.
    #[inline]
    pub fn target_mut(&mut self) -> &mut TTarget {
        &mut self.target
    }

    /// After `poll()` returned `SendRpc`, this method should be called when the node sends back
    /// the result of the query.
    ///
    /// Note that if this query is a `FindValue` query and a node returns a record, feel free to
    /// immediately drop the query altogether and use the record.
    ///
    /// After this function returns, you should call `poll()` again.
    pub fn inject_rpc_result(
        &mut self,
        result_source: &impl PartialEq<TPeerId>,
        closer_peers: impl IntoIterator<Item = TPeerId>,
    ) {
        // Mark the peer as succeeded.
        for (peer_id, state) in self.closest_peers.iter_mut() {
            if result_source == peer_id {
                if let state @ QueryPeerState::InProgress(_) = state {
                    *state = QueryPeerState::Succeeded;
                }
            }
        }

        // Add the entries in `closest_peers`.
        if let QueryStage::Iterating {
            ref mut no_closer_in_a_row,
        } = self.stage
        {
            // We increment now, and reset to 0 if we find a closer node.
            *no_closer_in_a_row += 1;

            for elem_to_add in closer_peers {
                let target = &self.target;
                let elem_to_add_distance = target.distance_with(&elem_to_add);
                let insert_pos_start = self.closest_peers.iter().position(|(id, _)| {
                    target.distance_with(&id) >= elem_to_add_distance
                });

                if let Some(insert_pos_start) = insert_pos_start {
                    // We need to insert the element between `insert_pos_start` and
                    // `insert_pos_start + insert_pos_size`.
                    let insert_pos_size = self.closest_peers.iter()
                        .skip(insert_pos_start)
                        .position(|(id, _)| {
                            target.distance_with(&id) > elem_to_add_distance
                        });

                    // Make sure we don't insert duplicates.
                    let duplicate = if let Some(insert_pos_size) = insert_pos_size {
                        self.closest_peers.iter().skip(insert_pos_start).take(insert_pos_size).any(|e| e.0 == elem_to_add)
                    } else {
                        self.closest_peers.iter().skip(insert_pos_start).any(|e| e.0 == elem_to_add)
                    };

                    if !duplicate {
                        if insert_pos_start == 0 {
                            *no_closer_in_a_row = 0;
                        }
                        debug_assert!(self.closest_peers.iter().all(|e| e.0 != elem_to_add));
                        self.closest_peers
                            .insert(insert_pos_start, (elem_to_add, QueryPeerState::NotContacted));
                    }
                } else if self.closest_peers.len() < self.num_results {
                    debug_assert!(self.closest_peers.iter().all(|e| e.0 != elem_to_add));
                    self.closest_peers
                        .push((elem_to_add, QueryPeerState::NotContacted));
                }
            }
        }

        // Check for duplicates in `closest_peers`.
        debug_assert!(self.closest_peers.windows(2).all(|w| w[0].0 != w[1].0));

        // Handle if `no_closer_in_a_row` is too high.
        let freeze = if let QueryStage::Iterating { no_closer_in_a_row } = self.stage {
            no_closer_in_a_row >= self.parallelism
        } else {
            false
        };
        if freeze {
            self.stage = QueryStage::Frozen;
        }
    }

    /// Returns the list of peers for which we are waiting for an answer.
    pub fn waiting(&self) -> impl Iterator<Item = &TPeerId> {
        self.closest_peers
            .iter()
            .filter(|(_, state)| {
                match state {
                    QueryPeerState::InProgress(_) => true,
                    QueryPeerState::NotContacted => false,
                    QueryPeerState::Succeeded => false,
                    QueryPeerState::Failed => false,
                }
            })
            .map(|(id, _)| id)
    }

    /// Returns true if we are waiting for a query answer from that peer.
    ///
    /// After `poll()` returned `SendRpc`, this function will return `true`.
    pub fn is_waiting(&self, id: &impl PartialEq<TPeerId>) -> bool {
        self.waiting().any(|peer_id| id == peer_id)
    }

    /// After `poll()` returned `SendRpc`, this function should be called if we were unable to
    /// reach the peer, or if an error of some sort happened.
    ///
    /// Has no effect if the peer ID is not relevant to the query, so feel free to call this
    /// function whenever an error happens on the network.
    ///
    /// After this function returns, you should call `poll()` again.
    pub fn inject_rpc_error(&mut self, id: &TPeerId) {
        let state = self
            .closest_peers
            .iter_mut()
            .filter_map(
                |(peer_id, state)| {
                    if peer_id == id {
                        Some(state)
                    } else {
                        None
                    }
                },
            )
            .next();

        match state {
            Some(state @ &mut QueryPeerState::InProgress(_)) => *state = QueryPeerState::Failed,
            Some(&mut QueryPeerState::NotContacted) => (),
            Some(&mut QueryPeerState::Succeeded) => (),
            Some(&mut QueryPeerState::Failed) => (),
            None => (),
        }
    }

    /// Polls this individual query.
    pub fn poll(&mut self) -> Async<QueryStatePollOut<'_, TTarget, TPeerId>> {
        // While iterating over peers, count the number of queries currently being processed.
        // This is used to not go over the limit of parallel requests.
        // If this is still 0 at the end of the function, that means the query is finished.
        let mut active_counter = 0;

        // While iterating over peers, count the number of queries in a row (from closer to further
        // away from target) that are in the succeeded in state.
        // Contains `None` if the chain is broken.
        let mut succeeded_counter = Some(0);

        // Extract `self.num_results` to avoid borrowing errors with closures.
        let num_results = self.num_results;

        for &mut (ref peer_id, ref mut state) in self.closest_peers.iter_mut() {
            // Start by "killing" the query if it timed out.
            {
                let timed_out = match state {
                    QueryPeerState::InProgress(timeout) => match timeout.poll() {
                        Ok(Async::Ready(_)) | Err(_) => true,
                        Ok(Async::NotReady) => false,
                    },
                    _ => false,
                };
                if timed_out {
                    *state = QueryPeerState::Failed;
                    return Async::Ready(QueryStatePollOut::CancelRpc { peer_id });
                }
            }

            // Increment the local counters.
            match state {
                QueryPeerState::InProgress(_) => {
                    active_counter += 1;
                }
                QueryPeerState::Succeeded => {
                    if let Some(ref mut c) = succeeded_counter {
                        *c += 1;
                    }
                }
                _ => (),
            };

            // We have enough results; the query is done.
            if succeeded_counter
                .as_ref()
                .map(|&c| c >= num_results)
                .unwrap_or(false)
            {
                return Async::Ready(QueryStatePollOut::Finished);
            }

            // Dial the node if it needs dialing.
            let need_connect = match state {
                QueryPeerState::NotContacted => match self.stage {
                    QueryStage::Iterating { .. } => active_counter < self.parallelism,
                    QueryStage::Frozen => true,     // TODO: as an optimization, could be false if we're not trying to find peers
                },
                _ => false,
            };

            if need_connect {
                let delay = Delay::new(Instant::now() + self.rpc_timeout);
                *state = QueryPeerState::InProgress(delay);
                return Async::Ready(QueryStatePollOut::SendRpc {
                    peer_id,
                    query_target: &self.target,
                });
            }
        }

        // If we don't have any query in progress, return `Finished` as we don't have anything more
        // we can do.
        if active_counter > 0 {
            Async::NotReady
        } else {
            Async::Ready(QueryStatePollOut::Finished)
        }
    }

    /// Consumes the query and returns the targe tand known closest peers.
    ///
    /// > **Note**: This can be called at any time, but you normally only do that once the query
    /// >           is finished.
    pub fn into_target_and_closest_peers(self) -> (TTarget, impl Iterator<Item = TPeerId>) {
        let closest = self.closest_peers
            .into_iter()
            .filter_map(|(peer_id, state)| {
                if let QueryPeerState::Succeeded = state {
                    Some(peer_id)
                } else {
                    None
                }
            })
            .take(self.num_results);
        (self.target, closest)
    }

    /// Consumes the query and returns the known closest peers.
    ///
    /// > **Note**: This can be called at any time, but you normally only do that once the query
    /// >           is finished.
    pub fn into_closest_peers(self) -> impl Iterator<Item = TPeerId> {
        self.into_target_and_closest_peers().1
    }
}

/// Outcome of polling a query.
#[derive(Debug, Clone)]
pub enum QueryStatePollOut<'a, TTarget, TPeerId> {
    /// The query is finished.
    ///
    /// If this is a `FindValue` query, the user is supposed to extract the record themselves from
    /// any RPC result sent by a remote. If the query finished without that happening, this means
    /// that we didn't find any record.
    /// Similarly, if this is a `GetProviders` query, the user is supposed to extract the providers
    /// from any RPC result sent by a remote.
    ///
    /// If this is a `FindNode` query, you can call `into_closest_peers` in order to obtain the
    /// result.
    Finished,

    /// We need to send an RPC query to the given peer.
    ///
    /// The RPC query to send can be derived from the target of the query.
    ///
    /// After this has been returned, you should call either `inject_rpc_result` or
    /// `inject_rpc_error` at a later point in time.
    SendRpc {
        /// The peer to send the RPC query to.
        peer_id: &'a TPeerId,
        /// A reminder of the query target. Same as what you obtain by calling `target()`.
        query_target: &'a TTarget,
    },

    /// We no longer need to send a query to this specific node.
    ///
    /// It is guaranteed that an earlier polling returned `SendRpc` with this peer id.
    CancelRpc {
        /// The target.
        peer_id: &'a TPeerId,
    },
}

/// State of peer in the context of a query.
#[derive(Debug)]
enum QueryPeerState {
    /// We haven't tried contacting the node.
    NotContacted,
    /// Waiting for an answer from the node to our RPC query. Includes a timeout.
    InProgress(Delay),
    /// We successfully reached the node.
    Succeeded,
    /// We tried to reach the node but failed.
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{QueryConfig, QueryState, QueryStatePollOut};
    use futures::{self, try_ready, prelude::*};
    use libp2p_core::PeerId;
    use std::{iter, time::Duration, sync::Arc, sync::Mutex, thread};
    use tokio;

    #[test]
    fn start_by_sending_rpc_to_known_peers() {
        let random_id = PeerId::random();
        let target = PeerId::random();

        let mut query = QueryState::new(QueryConfig {
            target,
            known_closest_peers: iter::once(random_id.clone()),
            parallelism: 3,
            num_results: 100,
            rpc_timeout: Duration::from_secs(10),
        });

        tokio::run(futures::future::poll_fn(move || {
            match try_ready!(Ok(query.poll())) {
                QueryStatePollOut::SendRpc { peer_id, .. } if peer_id == &random_id => {
                    Ok(Async::Ready(()))
                }
                _ => panic!(),
            }
        }));
    }

    #[test]
    fn continue_second_result() {
        let random_id = PeerId::random();
        let random_id2 = PeerId::random();
        let target = PeerId::random();

        let query = Arc::new(Mutex::new(QueryState::new(QueryConfig {
            target,
            known_closest_peers: iter::once(random_id.clone()),
            parallelism: 3,
            num_results: 100,
            rpc_timeout: Duration::from_secs(10),
        })));

        // Let's do a first polling round to obtain the `SendRpc` request.
        tokio::run(futures::future::poll_fn({
            let random_id = random_id.clone();
            let query = query.clone();
            move || {
                match try_ready!(Ok(query.lock().unwrap().poll())) {
                    QueryStatePollOut::SendRpc { peer_id, .. } if peer_id == &random_id => {
                        Ok(Async::Ready(()))
                    }
                    _ => panic!(),
                }
            }
        }));

        // Send the reply.
        query.lock().unwrap().inject_rpc_result(&random_id, iter::once(random_id2.clone()));

        // Second polling round to check the second `SendRpc` request.
        tokio::run(futures::future::poll_fn({
            let query = query.clone();
            move || {
                match try_ready!(Ok(query.lock().unwrap().poll())) {
                    QueryStatePollOut::SendRpc { peer_id, .. } if peer_id == &random_id2 => {
                        Ok(Async::Ready(()))
                    }
                    _ => panic!(),
                }
            }
        }));
    }

    #[test]
    fn timeout_works() {
        let random_id = PeerId::random();

        let query = Arc::new(Mutex::new(QueryState::new(QueryConfig {
            target: PeerId::random(),
            known_closest_peers: iter::once(random_id.clone()),
            parallelism: 3,
            num_results: 100,
            rpc_timeout: Duration::from_millis(100),
        })));

        // Let's do a first polling round to obtain the `SendRpc` request.
        tokio::run(futures::future::poll_fn({
            let random_id = random_id.clone();
            let query = query.clone();
            move || {
                match try_ready!(Ok(query.lock().unwrap().poll())) {
                    QueryStatePollOut::SendRpc { peer_id, .. } if peer_id == &random_id => {
                        Ok(Async::Ready(()))
                    }
                    _ => panic!(),
                }
            }
        }));

        // Wait for a bit.
        thread::sleep(Duration::from_millis(200));

        // Second polling round to check the timeout.
        tokio::run(futures::future::poll_fn({
            let query = query.clone();
            move || {
                match try_ready!(Ok(query.lock().unwrap().poll())) {
                    QueryStatePollOut::CancelRpc { peer_id, .. } if peer_id == &random_id => {
                        Ok(Async::Ready(()))
                    }
                    _ => panic!(),
                }
            }
        }));

        // Third polling round for finished.
        tokio::run(futures::future::poll_fn({
            let query = query.clone();
            move || {
                match try_ready!(Ok(query.lock().unwrap().poll())) {
                    QueryStatePollOut::Finished => {
                        Ok(Async::Ready(()))
                    }
                    _ => panic!(),
                }
            }
        }));
    }
}
