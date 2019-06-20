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

//! A state machine for an iterative Kademlia query.
//!
//! Using a [`Query`] typically involves performing the following steps
//! repeatedly and in an alternating fashion:
//!
//!   1. Calling [`next`] to observe the next state of the query and determine
//!      what to do, which is to either issue new requests to peers or continue
//!      waiting for responses.
//!
//!   2. When responses are received or requests fail, providing input to the
//!      query via the [`on_success`] and [`on_failure`] callbacks,
//!      respectively, followed by repeating step (1).
//!
//! When a call to [`next`] returns [`Finished`], the query is finished and the
//! resulting closest peers can be obtained from [`into_result`].
//!
//! A query can be finished prematurely at any time through [`finish`]
//! (e.g. if a response to a `FIND_VALUE` request returns the value being
//! searched for).
//!
//! [`Query`]: query::Query
//! [`next`]: query::Query::next
//! [`finish`]: query::Query::finish
//! [`on_success`]: query::Query::on_success
//! [`on_failure`]: query::Query::on_failure
//! [`into_result`]: query::Query::into_result
//! [`Finished`]: query::QueryState::Finished

use crate::kbucket::{Key, Distance, MAX_NODES_PER_BUCKET};
use std::{time::Duration, iter::FromIterator};
use std::collections::btree_map::{BTreeMap, Entry};
use wasm_timer::Instant;

/// A `Query` is a state machine for an iterative Kademlia query.
#[derive(Debug, Clone)]
pub struct Query<TTarget, TPeerId> {
    /// The configuration of the query.
    config: QueryConfig,

    /// The target of the query.
    target: TTarget,

    /// The target of the query as a `Key`.
    target_key: Key<TTarget>,

    /// The current state of progress of the query.
    progress: QueryProgress,

    /// The closest peers to the target, ordered by increasing distance.
    closest_peers: BTreeMap<Distance, QueryPeer<TPeerId>>,

    /// The number of peers for which the query is currently waiting for results.
    num_waiting: usize,
}

/// Configuration for a `Query`.
#[derive(Debug, Clone)]
pub struct QueryConfig {
    /// Allowed level of parallelism.
    ///
    /// The `α` parameter in the Kademlia paper. The maximum number of peers that a query
    /// is allowed to wait for in parallel while iterating towards the closest
    /// nodes to a target. Defaults to `3`.
    pub parallelism: usize,

    /// Number of results to produce.
    ///
    /// The number of closest peers that a query must obtain successful results
    /// for before it terminates. Defaults to the maximum number of entries in a
    /// single k-bucket, i.e. the `k` parameter in the Kademlia paper.
    pub num_results: usize,

    /// The timeout for a single peer.
    ///
    /// If a successful result is not reported for a peer within this timeout
    /// window, a query considers the peer unresponsive and will not consider
    /// the peer when evaluating the termination conditions of a query until
    /// and unless it responds. Defaults to `10` seconds.
    pub peer_timeout: Duration,
}

impl Default for QueryConfig {
    fn default() -> Self {
        QueryConfig {
            parallelism: 3,
            peer_timeout: Duration::from_secs(10),
            num_results: MAX_NODES_PER_BUCKET
        }
    }
}

/// The result of a `Query`.
pub struct QueryResult<TTarget, TClosest> {
    /// The target of the query.
    pub target: TTarget,
    /// The closest peers to the target found by the query.
    pub closest_peers: TClosest
}

/// The state of the query reported by [`Query::next`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryState<'a, TPeerId> {
    /// The query is waiting for results.
    ///
    /// `Some(peer)` indicates that the query is now waiting for a result
    /// from `peer`, in addition to any other peers for which the query is already
    /// waiting for results.
    ///
    /// `None` indicates that the query is waiting for results and there is no
    /// new peer to contact, despite the query not being at capacity w.r.t.
    /// the permitted parallelism.
    Waiting(Option<&'a TPeerId>),

    /// The query is waiting for results and is at capacity w.r.t. the
    /// permitted parallelism.
    WaitingAtCapacity,

    /// The query finished.
    Finished
}

impl<TTarget, TPeerId> Query<TTarget, TPeerId>
where
    TTarget: Into<Key<TTarget>> + Clone,
    TPeerId: Into<Key<TPeerId>> + Eq + Clone
{
    /// Creates a new query with a default configuration.
    pub fn new<I>(target: TTarget, known_closest_peers: I) -> Self
    where
        I: IntoIterator<Item = Key<TPeerId>>
    {
        Self::with_config(QueryConfig::default(), target, known_closest_peers)
    }

    /// Creates a new query with the given configuration.
    pub fn with_config<I>(config: QueryConfig, target: TTarget, known_closest_peers: I) -> Self
    where
        I: IntoIterator<Item = Key<TPeerId>>
    {
        let target_key = target.clone().into();

        // Initialise the closest peers to begin the query with.
        let closest_peers = BTreeMap::from_iter(
            known_closest_peers
                .into_iter()
                .map(|key| {
                    let distance = key.distance(&target_key);
                    let state = QueryPeerState::NotContacted;
                    (distance, QueryPeer { key, state })
                })
                .take(config.num_results));

        // The query initially makes progress by iterating towards the target.
        let progress = QueryProgress::Iterating { no_progress : 0 };

        Query {
            config,
            target,
            target_key,
            progress,
            closest_peers,
            num_waiting: 0
        }
    }

    /// Borrows the underlying target of the query.
    pub fn target(&self) -> &TTarget {
        &self.target
    }

    /// Mutably borrows the underlying target of the query.
    pub fn target_mut(&mut self) -> &mut TTarget {
        &mut self.target
    }

    /// Callback for delivering the result of a successful request to a peer
    /// that the query is waiting on.
    ///
    /// Delivering results of requests back to the query allows the query to make
    /// progress. The query is said to make progress either when the given
    /// `closer_peers` contain a peer closer to the target than any peer seen so far,
    /// or when the query did not yet accumulate `num_results` closest peers and
    /// `closer_peers` contains a new peer, regardless of its distance to the target.
    ///
    /// After calling this function, `next` should eventually be called again
    /// to advance the state of the query.
    ///
    /// If the query is finished, the query is not currently waiting for a
    /// result from `peer`, or a result for `peer` has already been reported,
    /// calling this function has no effect.
    pub fn on_success<I>(&mut self, peer: &TPeerId, closer_peers: I)
    where
        I: IntoIterator<Item = TPeerId>
    {
        if let QueryProgress::Finished = self.progress {
            return
        }

        let key = peer.clone().into();
        let distance = key.distance(&self.target_key);

        // Mark the peer as succeeded.
        match self.closest_peers.entry(distance) {
            Entry::Vacant(..) => return,
            Entry::Occupied(mut e) => match e.get().state {
                QueryPeerState::Waiting(..) => {
                    debug_assert!(self.num_waiting > 0);
                    self.num_waiting -= 1;
                    e.get_mut().state = QueryPeerState::Succeeded;
                }
                QueryPeerState::Unresponsive => {
                    e.get_mut().state = QueryPeerState::Succeeded;
                }
                QueryPeerState::NotContacted
                    | QueryPeerState::Failed
                    | QueryPeerState::Succeeded => return
            }
        }

        let num_closest = self.closest_peers.len();
        let mut progress = false;

        // Incorporate the reported closer peers into the query.
        for peer in closer_peers {
            let key = peer.into();
            let distance = self.target_key.distance(&key);
            let peer = QueryPeer { key, state: QueryPeerState::NotContacted };
            self.closest_peers.entry(distance).or_insert(peer);
            // The query makes progress if the new peer is either closer to the target
            // than any peer seen so far (i.e. is the first entry), or the query did
            // not yet accumulate enough closest peers.
            progress = self.closest_peers.keys().next() == Some(&distance)
                || num_closest < self.config.num_results;
        }

        // Update the query progress.
        self.progress = match self.progress {
            QueryProgress::Iterating { no_progress } => {
                let no_progress = if progress { 0 } else { no_progress + 1 };
                if no_progress >= self.config.parallelism {
                    QueryProgress::Stalled
                } else {
                    QueryProgress::Iterating { no_progress }
                }
            }
            QueryProgress::Stalled =>
                if progress {
                    QueryProgress::Iterating { no_progress: 0 }
                } else {
                    QueryProgress::Stalled
                }
            QueryProgress::Finished => QueryProgress::Finished
        }
    }

    /// Callback for informing the query about a failed request to a peer
    /// that the query is waiting on.
    ///
    /// After calling this function, `next` should eventually be called again
    /// to advance the state of the query.
    ///
    /// If the query is finished, the query is not currently waiting for a
    /// result from `peer`, or a result for `peer` has already been reported,
    /// calling this function has no effect.
    pub fn on_failure(&mut self, peer: &TPeerId) {
        if let QueryProgress::Finished = self.progress {
            return
        }

        let key = peer.clone().into();
        let distance = key.distance(&self.target_key);

        match self.closest_peers.entry(distance) {
            Entry::Vacant(_) => return,
            Entry::Occupied(mut e) => match e.get().state {
                QueryPeerState::Waiting(_) => {
                    debug_assert!(self.num_waiting > 0);
                    self.num_waiting -= 1;
                    e.get_mut().state = QueryPeerState::Failed
                }
                QueryPeerState::Unresponsive => {
                    e.get_mut().state = QueryPeerState::Failed
                }
                _ => {}
            }
        }
    }

    /// Returns the list of peers for which the query is currently waiting
    /// for results.
    pub fn waiting(&self) -> impl Iterator<Item = &TPeerId> {
        self.closest_peers.values().filter_map(|peer|
            match peer.state {
                QueryPeerState::Waiting(..) => Some(peer.key.preimage()),
                _ => None
            })
    }

    /// Returns the number of peers for which the query is currently
    /// waiting for results.
    pub fn num_waiting(&self) -> usize {
        self.num_waiting
    }

    /// Returns true if the query is waiting for a response from the given peer.
    pub fn is_waiting(&self, peer: &TPeerId) -> bool {
        self.waiting().any(|p| peer == p)
    }

    /// Advances the state of the query, potentially getting a new peer to contact.
    ///
    /// See [`QueryState`].
    pub fn next(&mut self, now: Instant) -> QueryState<TPeerId> {
        if let QueryProgress::Finished = self.progress {
            return QueryState::Finished
        }

        // Count the number of peers that returned a result. If there is a
        // request in progress to one of the `num_results` closest peers, the
        // counter is set to `None` as the query can only finish once
        // `num_results` closest peers have responded (or there are no more
        // peers to contact, see `active_counter`).
        let mut result_counter = Some(0);

        // Check if the query is at capacity w.r.t. the allowed parallelism.
        let at_capacity = self.at_capacity();

        for peer in self.closest_peers.values_mut() {
            match peer.state {
                QueryPeerState::Waiting(timeout) => {
                    if now >= timeout {
                        // Unresponsive peers no longer count towards the limit for the
                        // bounded parallelism, though they might still be ongoing and
                        // their results can still be delivered to the query.
                        debug_assert!(self.num_waiting > 0);
                        self.num_waiting -= 1;
                        peer.state = QueryPeerState::Unresponsive
                    }
                    else if at_capacity {
                        // The query is still waiting for a result from a peer and is
                        // at capacity w.r.t. the maximum number of peers being waited on.
                        return QueryState::WaitingAtCapacity
                    }
                    else {
                        // The query is still waiting for a result from a peer and the
                        // `result_counter` did not yet reach `num_results`. Therefore
                        // the query is not yet done, regardless of already successful
                        // queries to peers farther from the target.
                        result_counter = None;
                    }
                }

                QueryPeerState::Succeeded =>
                    if let Some(ref mut cnt) = result_counter {
                        *cnt += 1;
                        // If `num_results` successful results have been delivered for the
                        // closest peers, the query is done.
                        if *cnt >= self.config.num_results {
                            self.progress = QueryProgress::Finished;
                            return QueryState::Finished
                        }
                    }

                QueryPeerState::NotContacted =>
                    if !at_capacity {
                        let timeout = now + self.config.peer_timeout;
                        peer.state = QueryPeerState::Waiting(timeout);
                        self.num_waiting += 1;
                        return QueryState::Waiting(Some(peer.key.preimage()))
                    } else {
                        return QueryState::WaitingAtCapacity
                    }

                QueryPeerState::Unresponsive | QueryPeerState::Failed => {
                    // Skip over unresponsive or failed peers.
                }
            }
        }

        if self.num_waiting > 0 {
            // The query is still waiting for results and not at capacity w.r.t.
            // the allowed parallelism, but there are no new peers to contact
            // at the moment.
            QueryState::Waiting(None)
        } else {
            // The query is finished because all available peers have been contacted
            // and the query is not waiting for any more results.
            self.progress = QueryProgress::Finished;
            QueryState::Finished
        }
    }

    /// Immediately transitions the query to [`QueryState::Finished`].
    pub fn finish(&mut self) {
        self.progress = QueryProgress::Finished
    }

    /// Checks whether the query has finished.
    pub fn finished(&self) -> bool {
        self.progress == QueryProgress::Finished
    }

    /// Consumes the query, returning the target and the closest peers.
    pub fn into_result(self) -> QueryResult<TTarget, impl Iterator<Item = TPeerId>> {
        let closest_peers = self.closest_peers
            .into_iter()
            .filter_map(|(_, peer)| {
                if let QueryPeerState::Succeeded = peer.state {
                    Some(peer.key.into_preimage())
                } else {
                    None
                }
            })
            .take(self.config.num_results);

        QueryResult { target: self.target, closest_peers }
    }

    /// Checks if the query is at capacity w.r.t. the permitted parallelism.
    ///
    /// While the query is stalled, up to `num_results` parallel requests
    /// are allowed. This is a slightly more permissive variant of the
    /// requirement that the initiator "resends the FIND_NODE to all of the
    /// k closest nodes it has not already queried".
    fn at_capacity(&self) -> bool {
        match self.progress {
            QueryProgress::Stalled => self.num_waiting >= self.config.num_results,
            QueryProgress::Iterating { .. } => self.num_waiting >= self.config.parallelism,
            QueryProgress::Finished => true
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Private state

/// Stage of the query.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum QueryProgress {
    /// The query is making progress by iterating towards `num_results` closest
    /// peers to the target with a maximum of `parallelism` peers for which the
    /// query is waiting for results at a time.
    ///
    /// > **Note**: When the query switches back to `Iterating` after being
    /// > `Stalled`, it may temporarily be waiting for more than `parallelism`
    /// > results from peers, with new peers only being considered once
    /// > the number pending results drops below `parallelism`.
    Iterating {
        /// The number of consecutive results that did not yield a peer closer
        /// to the target. When this number reaches `parallelism` and no new
        /// peer was discovered or at least `num_results` peers are known to
        /// the query, it is considered `Stalled`.
        no_progress: usize,
    },

    /// A query is stalled when it did not make progress after `parallelism`
    /// consecutive successful results (see `on_success`).
    ///
    /// While the query is stalled, the maximum allowed parallelism for pending
    /// results is increased to `num_results` in an attempt to finish the query.
    /// If the query can make progress again upon receiving the remaining
    /// results, it switches back to `Iterating`. Otherwise it will be finished.
    Stalled,

    /// The query is finished.
    ///
    /// A query finishes either when it has collected `num_results` results
    /// from the closest peers (not counting those that failed or are unresponsive)
    /// or because the query ran out of peers that have not yet delivered
    /// results (or failed).
    Finished
}

/// Representation of a peer in the context of a query.
#[derive(Debug, Clone)]
struct QueryPeer<TPeerId> {
    key: Key<TPeerId>,
    state: QueryPeerState
}

/// The state of `QueryPeer` in the context of a query.
#[derive(Debug, Copy, Clone)]
enum QueryPeerState {
    /// The peer has not yet been contacted.
    ///
    /// This is the starting state for every peer known to, or discovered by, a query.
    NotContacted,

    /// The query is waiting for a result from the peer.
    Waiting(Instant),

    /// A result was not delivered for the peer within the configured timeout.
    ///
    /// The peer is not taken into account for the termination conditions
    /// of the query until and unless it responds.
    Unresponsive,

    /// Obtaining a result from the peer has failed.
    ///
    /// This is a final state, reached as a result of a call to `on_failure`.
    Failed,

    /// A successful result from the peer has been delivered.
    ///
    /// This is a final state, reached as a result of a call to `on_success`.
    Succeeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::PeerId;
    use quickcheck::*;
    use rand::{Rng, thread_rng};
    use std::{iter, time::Duration};

    type TestQuery = Query<PeerId, PeerId>;

    fn random_peers(n: usize) -> impl Iterator<Item = PeerId> + Clone {
        (0 .. n).map(|_| PeerId::random())
    }

    fn random_query<G: Rng>(g: &mut G) -> TestQuery {
        let known_closest_peers = random_peers(g.gen_range(1, 60)).map(Key::from);
        let target = PeerId::random();
        let config = QueryConfig {
            parallelism: g.gen_range(1, 10),
            num_results: g.gen_range(1, 25),
            peer_timeout: Duration::from_secs(g.gen_range(10, 30)),
        };
        Query::with_config(config, target, known_closest_peers)
    }

    fn sorted(target: &Key<PeerId>, peers: &Vec<Key<PeerId>>) -> bool {
        peers.windows(2).all(|w| w[0].distance(&target) < w[1].distance(&target))
    }

    impl Arbitrary for TestQuery {
        fn arbitrary<G: Gen>(g: &mut G) -> TestQuery {
            random_query(g)
        }
    }

    #[test]
    fn new_query() {
        let query = random_query(&mut thread_rng());
        let target = Key::from(query.target().clone());

        let (keys, states): (Vec<_>, Vec<_>) = query.closest_peers
            .values()
            .map(|e| (e.key.clone(), &e.state))
            .unzip();

        let none_contacted = states
            .iter()
            .all(|s| match s {
                QueryPeerState::NotContacted => true,
                _ => false
            });

        assert!(none_contacted,
            "Unexpected peer state in new query.");
        assert!(sorted(&target, &keys),
            "Closest peers in new query not sorted by distance to target.");
        assert_eq!(query.num_waiting(), 0,
            "Unexpected peers in progress in new query.");
        assert_eq!(query.into_result().closest_peers.count(), 0,
            "Unexpected closest peers in new query");
    }

    #[test]
    fn termination_and_parallelism() {
        fn prop(mut query: TestQuery) {
            let now = Instant::now();
            let mut rng = thread_rng();

            let mut expected = query.closest_peers
                .values()
                .map(|e| e.key.clone())
                .collect::<Vec<_>>();
            let num_known = expected.len();
            let max_parallelism = usize::min(query.config.parallelism, num_known);

            let target = Key::from(query.target().clone());
            let mut remaining;
            let mut num_failures = 0;

            'finished: loop {
                if expected.len() == 0 {
                    break;
                }
                // Split off the next up to `parallelism` expected peers.
                else if expected.len() < max_parallelism {
                    remaining = Vec::new();
                }
                else {
                    remaining = expected.split_off(max_parallelism);
                }

                // Advance the query for maximum parallelism.
                for k in expected.iter() {
                    match query.next(now) {
                        QueryState::Finished => break 'finished,
                        QueryState::Waiting(Some(p)) => assert_eq!(p, k.preimage()),
                        QueryState::Waiting(None) => panic!("Expected another peer."),
                        QueryState::WaitingAtCapacity => panic!("Unexpectedly reached capacity.")
                    }
                }
                let num_waiting = query.num_waiting();
                assert_eq!(num_waiting, expected.len());

                // Check the bounded parallelism.
                if query.at_capacity() {
                    assert_eq!(query.next(now), QueryState::WaitingAtCapacity)
                }

                // Report results back to the query with a random number of "closer"
                // peers or an error, thus finishing the "in-flight requests".
                for (i, k) in expected.iter().enumerate() {
                    if rng.gen_bool(0.75) {
                        let num_closer = rng.gen_range(0, query.config.num_results + 1);
                        let closer_peers = random_peers(num_closer).collect::<Vec<_>>();
                        remaining.extend(closer_peers.iter().cloned().map(Key::from));
                        query.on_success(k.preimage(), closer_peers);
                    } else {
                        num_failures += 1;
                        query.on_failure(k.preimage());
                    }
                    assert_eq!(query.num_waiting(), num_waiting - (i + 1));
                }

                // Re-sort the remaining expected peers for the next "round".
                remaining.sort_by_key(|k| target.distance(&k));

                expected = remaining
            }

            // The query must be finished.
            assert_eq!(query.next(now), QueryState::Finished);
            assert_eq!(query.progress, QueryProgress::Finished);

            // Determine if all peers have been contacted by the query. This _must_ be
            // the case if the query finished with fewer than the requested number
            // of results.
            let all_contacted = query.closest_peers.values().all(|e| match e.state {
                QueryPeerState::NotContacted | QueryPeerState::Waiting { .. } => false,
                _ => true
            });

            let target = query.target().clone();
            let num_results = query.config.num_results;
            let result = query.into_result();
            let closest = result.closest_peers.map(Key::from).collect::<Vec<_>>();

            assert_eq!(result.target, target);
            assert!(sorted(&Key::from(target), &closest));

            if closest.len() < num_results {
                // The query returned fewer results than requested. Therefore
                // either the initial number of known peers must have been
                // less than the desired number of results, or there must
                // have been failures.
                assert!(num_known < num_results || num_failures > 0);
                // All peers must have been contacted.
                assert!(all_contacted, "Not all peers have been contacted.");
            } else {
                assert_eq!(num_results, closest.len(), "Too  many results.");
            }
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_) -> _)
    }

    #[test]
    fn no_duplicates() {
        fn prop(mut query: TestQuery) -> bool {
            let now = Instant::now();
            let closer = random_peers(1).collect::<Vec<_>>();

            // A first peer reports a "closer" peer.
            let peer1 = match query.next(now) {
                QueryState::Waiting(Some(p)) => p.clone(),
                _ => panic!("No peer.")
            };
            query.on_success(&peer1, closer.clone());
            // Duplicate result from te same peer.
            query.on_success(&peer1, closer.clone());

            // If there is a second peer, let it also report the same "closer" peer.
            match query.next(now) {
                QueryState::Waiting(Some(p)) => {
                    let peer2 = p.clone();
                    query.on_success(&peer2, closer.clone())
                }
                QueryState::Finished => {}
                _ => panic!("Unexpectedly query state."),
            };

            // The "closer" peer must only be in the query once.
            let n = query.closest_peers.values().filter(|e| e.key.preimage() == &closer[0]).count();
            assert_eq!(n, 1);

            true
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_) -> _)
    }

    #[test]
    fn timeout() {
        fn prop(mut query: TestQuery) -> bool {
            let mut now = Instant::now();
            let peer = query.closest_peers.values().next().unwrap().key.clone().into_preimage();

            // Poll the query for the first peer to be in progress.
            match query.next(now) {
                QueryState::Waiting(Some(id)) => assert_eq!(id, &peer),
                _ => panic!()
            }

            // Artificially advance the clock.
            now = now + query.config.peer_timeout;

            // Advancing the query again should mark the first peer as unresponsive.
            let _ = query.next(now);
            match &query.closest_peers.values().next().unwrap() {
                QueryPeer { key, state: QueryPeerState::Unresponsive } => {
                    assert_eq!(key.preimage(), &peer);
                },
                QueryPeer { state, .. } => panic!("Unexpected peer state: {:?}", state)
            }

            let finished = query.finished();
            query.on_success(&peer, iter::empty());
            let closest = query.into_result().closest_peers.collect::<Vec<_>>();

            if finished {
                // Delivering results when the query already finished must have
                // no effect.
                assert_eq!(Vec::<PeerId>::new(), closest)
            } else {
                // Unresponsive peers can still deliver results while the query
                // is not finished.
                assert_eq!(vec![peer], closest)
            }
            true
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_) -> _)
    }
}
