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

use super::*;

use crate::{K_VALUE, ALPHA_VALUE};
use crate::kbucket::{Key, KeyBytes, Distance};
use libp2p_core::PeerId;
use std::collections::btree_map::{BTreeMap, Entry};
use std::num::NonZeroU8;
use std::{time::Duration, iter::FromIterator};
use wasm_timer::Instant;

/// A peer iterator for a dynamically changing list of peers, sorted by increasing
/// distance to a chosen target.
#[derive(Debug, Clone)]
pub struct ClosestPeersIter {
    config: ClosestPeersIterConfig,

    /// The target whose distance to any peer determines the position of
    /// the peer in the iterator.
    target: KeyBytes,

    /// The internal iterator state.
    state: State,

    /// The closest peers to the target, ordered by increasing distance.
    closest_peers: BTreeMap<Distance, Peer>,

    /// The number of peers for which the iterator is currently waiting for results.
    num_waiting: usize,

    /// The next disjoint path to pursue.
    ///
    /// When the disjoint path feature is enabled (see
    /// [`ClosestPeersIterConfig::disjoint_paths`]) queries should try to explore different
    /// disjoint paths within the graph in a round robin fashion. `next_disjoint_path` tracks which
    /// path to continue exploring next.
    next_disjoint_path: PathId,
}

/// Configuration for a `ClosestPeersIter`.
#[derive(Debug, Clone)]
pub struct ClosestPeersIterConfig {
    /// Allowed level of parallelism.
    ///
    /// The `α` parameter in the Kademlia paper. The maximum number of peers that
    /// the iterator is allowed to wait for in parallel while iterating towards the closest
    /// nodes to a target. Defaults to `ALPHA_VALUE`.
    pub parallelism: usize,

    /// Number of results (closest peers) to search for.
    ///
    /// The number of closest peers for which the iterator must obtain successful results
    /// in order to finish successfully. Defaults to `K_VALUE`.
    pub num_results: usize,

    /// The timeout for a single peer.
    ///
    /// If a successful result is not reported for a peer within this timeout
    /// window, the iterator considers the peer unresponsive and will not wait for
    /// the peer when evaluating the termination conditions, until and unless a
    /// result is delivered. Defaults to `10` seconds.
    pub peer_timeout: Duration,

    /// The number of disjoint query paths to use.
    ///
    /// As described in the S-Kademlia paper: When exploring the network for a given target one can
    /// use disjoint paths to protect against adversarial nodes being along some, but not all paths.
    ///
    /// Setting `disjoint_paths` to `1` effectively disables the feature.
    pub disjoint_paths: NonZeroU8,
}

impl Default for ClosestPeersIterConfig {
    fn default() -> Self {
        ClosestPeersIterConfig {
            parallelism: ALPHA_VALUE.get(),
            num_results: K_VALUE.get(),
            peer_timeout: Duration::from_secs(10),
            disjoint_paths: unsafe { NonZeroU8::new_unchecked(1) },
        }
    }
}

impl ClosestPeersIter {
    /// Creates a new iterator with a default configuration.
    pub fn new<I>(target: KeyBytes, known_closest_peers: I) -> Self
    where
        I: IntoIterator<Item = Key<PeerId>>
    {
        Self::with_config(ClosestPeersIterConfig::default(), target, known_closest_peers)
    }

    /// Creates a new iterator with the given configuration.
    pub fn with_config<I, T>(config: ClosestPeersIterConfig, target: T, known_closest_peers: I) -> Self
    where
        I: IntoIterator<Item = Key<PeerId>>,
        T: Into<KeyBytes>
    {
        let target = target.into();

        // Initialise the closest peers to start the iterator with.
        let closest_peers = BTreeMap::from_iter(
            known_closest_peers
                .into_iter()
                .map(|key| {
                    let distance = key.distance(&target);
                    let state = PeerState::NotContacted(None);
                    (distance, Peer { key, state })
                })
                .take(config.num_results));

        // The iterator initially makes progress by iterating towards the target.
        let state = State::Iterating { no_progress : 0 };

        ClosestPeersIter {
            config,
            target,
            state,
            closest_peers,
            num_waiting: 0,
            next_disjoint_path: PathId(0)
        }
    }

    /// Callback for delivering the result of a successful request to a peer
    /// that the iterator is waiting on.
    ///
    /// Delivering results of requests back to the iterator allows the iterator to make
    /// progress. The iterator is said to make progress either when the given
    /// `closer_peers` contain a peer closer to the target than any peer seen so far,
    /// or when the iterator did not yet accumulate `num_results` closest peers and
    /// `closer_peers` contains a new peer, regardless of its distance to the target.
    ///
    /// After calling this function, `next` should eventually be called again
    /// to advance the state of the iterator.
    ///
    /// If the iterator is finished, it is not currently waiting for a
    /// result from `peer`, or a result for `peer` has already been reported,
    /// calling this function has no effect.
    pub fn on_success<I>(&mut self, peer: &PeerId, closer_peers: I)
    where
        I: IntoIterator<Item = PeerId>
    {
        if let State::Finished = self.state {
            return
        }

        let key = Key::from(peer.clone());
        let distance = key.distance(&self.target);
        let path_id;

        // Mark the peer as succeeded.
        match self.closest_peers.entry(distance) {
            Entry::Vacant(..) => return,
            Entry::Occupied(mut e) => match e.get().state {
                PeerState::Waiting((_, path)) => {
                    path_id = path;
                    debug_assert!(self.num_waiting > 0);
                    self.num_waiting -= 1;
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::Unresponsive(path) => {
                    path_id = path;
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::NotContacted(_)
                    | PeerState::Failed
                    | PeerState::Succeeded => return
            }
        }

        let num_closest = self.closest_peers.len();
        let mut progress = false;

        // Incorporate the reported closer peers into the iterator.
        for peer in closer_peers {
            let key = peer.into();
            let distance = self.target.distance(&key);
            let peer = Peer { key, state: PeerState::NotContacted(Some(path_id)) };
            self.closest_peers.entry(distance).or_insert(peer);
            // The iterator makes progress if the new peer is either closer to the target
            // than any peer seen so far (i.e. is the first entry), or the iterator did
            // not yet accumulate enough closest peers.
            progress = self.closest_peers.keys().next() == Some(&distance)
                || num_closest < self.config.num_results;
        }

        // Update the iterator state.
        self.state = match self.state {
            State::Iterating { no_progress } => {
                let no_progress = if progress { 0 } else { no_progress + 1 };
                if no_progress >= self.config.parallelism {
                    State::Stalled
                } else {
                    State::Iterating { no_progress }
                }
            }
            State::Stalled =>
                if progress {
                    State::Iterating { no_progress: 0 }
                } else {
                    State::Stalled
                }
            State::Finished => State::Finished
        }
    }

    /// Callback for informing the iterator about a failed request to a peer
    /// that the iterator is waiting on.
    ///
    /// After calling this function, `next` should eventually be called again
    /// to advance the state of the iterator.
    ///
    /// If the iterator is finished, it is not currently waiting for a
    /// result from `peer`, or a result for `peer` has already been reported,
    /// calling this function has no effect.
    pub fn on_failure(&mut self, peer: &PeerId) {
        if let State::Finished = self.state {
            return
        }

        let key = Key::from(peer.clone());
        let distance = key.distance(&self.target);

        match self.closest_peers.entry(distance) {
            Entry::Vacant(_) => return,
            Entry::Occupied(mut e) => match e.get().state {
                PeerState::Waiting(_) => {
                    debug_assert!(self.num_waiting > 0);
                    self.num_waiting -= 1;
                    e.get_mut().state = PeerState::Failed
                }
                PeerState::Unresponsive(_) => {
                    e.get_mut().state = PeerState::Failed
                }
                _ => {}
            }
        }
    }

    /// Returns the list of peers for which the iterator is currently waiting
    /// for results.
    pub fn waiting(&self) -> impl Iterator<Item = &PeerId> {
        self.closest_peers.values().filter_map(|peer|
            match peer.state {
                PeerState::Waiting(..) => Some(peer.key.preimage()),
                _ => None
            })
    }

    /// Returns the number of peers for which the iterator is currently
    /// waiting for results.
    pub fn num_waiting(&self) -> usize {
        self.num_waiting
    }

    /// Returns true if the iterator is waiting for a response from the given peer.
    pub fn is_waiting(&self, peer: &PeerId) -> bool {
        self.waiting().any(|p| peer == p)
    }


    /// Advances the state of the iterator, potentially getting a new peer to contact.
    pub fn next(&mut self, now: Instant) -> PeersIterState {
        if let State::Finished = self.state {
            return PeersIterState::Finished
        }

        // Count the number of peers that returned a result. If there is a
        // request in progress to one of the `num_results` closest peers, the
        // counter is set to `None` as the iterator can only finish once
        // `num_results` closest peers have responded (or there are no more
        // peers to contact, see `num_waiting`).
        let mut result_counter = Some(0);

        // Check if the iterator is at capacity w.r.t. the allowed parallelism.
        let at_capacity = self.at_capacity();

        // Check for timed-out peers, being at capacity or overall success.
        for peer in self.closest_peers.values_mut() {
            match peer.state {
                PeerState::Waiting((timeout, disjoint_path_id)) => {
                    if now >= timeout {
                        // Unresponsive peers no longer count towards the limit for the
                        // bounded parallelism, though they might still be ongoing and
                        // their results can still be delivered to the iterator.
                        debug_assert!(self.num_waiting > 0);
                        self.num_waiting -= 1;
                        peer.state = PeerState::Unresponsive(disjoint_path_id)
                    }
                    else if at_capacity {
                        // The iterator is still waiting for a result from a peer and is
                        // at capacity w.r.t. the maximum number of peers being waited on.
                        return PeersIterState::WaitingAtCapacity
                    }
                    else {
                        // The iterator is still waiting for a result from a peer and the
                        // `result_counter` did not yet reach `num_results`. Therefore
                        // the iterator is not yet done, regardless of already successful
                        // queries to peers farther from the target.
                        result_counter = None;
                    }
                }
                PeerState::Succeeded =>
                    if let Some(ref mut cnt) = result_counter {
                        *cnt += 1;
                        // If `num_results` successful results have been delivered for the
                        // closest peers, the iterator is done.
                        if *cnt >= self.config.num_results {
                            self.state = State::Finished;
                            return PeersIterState::Finished
                        }
                    }
                _ => {},
            }
        }

        let mut ignore_paths = u8::from(self.config.disjoint_paths) == 1;

        // Find a peer worth querying next.
        loop {
            for peer in self.closest_peers.values_mut() {
                if let PeerState::NotContacted(peer_path_id) = peer.state {
                    if at_capacity {
                        return PeersIterState::WaitingAtCapacity
                    }

                    let timeout = now + self.config.peer_timeout;

                    // If we care about disjoint paths, the given peer has a path and that path is
                    // not the one we are looking for, ignore the peer.
                    if let Some(path) = peer_path_id  {
                        if !ignore_paths && self.next_disjoint_path != path {
                            continue;
                        }
                    }

                    // Getting this far implies that we found a peer to query next, given that
                    // either:
                    //
                    // - Using disjoint paths is disabled, thereby ignoring paths.
                    //
                    // - The peer_path_id is None thereby the peer is from the initial set and thus
                    //   not part of a path yet.
                    //
                    // - The peer_path_id is equal to self.next_disjoint_path, thus exactly what we
                    //   are looking for.

                    peer.state = PeerState::Waiting((timeout, self.next_disjoint_path));
                    self.next_disjoint_path = PathId(
                        (self.next_disjoint_path.0 + 1) % (u8::from(self.config.disjoint_paths)),
                    );

                    self.num_waiting += 1;

                    // TODO: Shouldn't need to clone here. Compiler complaining about borrow
                    // from previous iteration.
                    return PeersIterState::Waiting(Some(Cow::Owned(peer.key.preimage().clone())))
                }
            }

            // We haven't found a peer worth querying next.
            if !ignore_paths {
                // Let's try again, ignoring usage of disjoint paths this time.
                ignore_paths = true;
            } else {
                // Let's give up for now.
                break;
            }
        }

        if self.num_waiting > 0 {
            // The iterator is still waiting for results and not at capacity w.r.t.
            // the allowed parallelism, but there are no new peers to contact
            // at the moment.
            PeersIterState::Waiting(None)
        } else {
            // The iterator is finished because all available peers have been contacted
            // and the iterator is not waiting for any more results.
            self.state = State::Finished;
            PeersIterState::Finished
        }
    }

    /// Immediately transitions the iterator to [`PeersIterState::Finished`].
    pub fn finish(&mut self) {
        self.state = State::Finished
    }

    /// Checks whether the iterator has finished.
    pub fn finished(&self) -> bool {
        self.state == State::Finished
    }

    /// Consumes the iterator, returning the target and the closest peers.
    pub fn into_result(self) -> impl Iterator<Item = PeerId> {
        self.closest_peers
            .into_iter()
            .filter_map(|(_, peer)| {
                if let PeerState::Succeeded = peer.state {
                    Some(peer.key.into_preimage())
                } else {
                    None
                }
            })
            .take(self.config.num_results)
    }

    /// Checks if the iterator is at capacity w.r.t. the permitted parallelism.
    ///
    /// While the iterator is stalled, up to `num_results` parallel requests
    /// are allowed. This is a slightly more permissive variant of the
    /// requirement that the initiator "resends the FIND_NODE to all of the
    /// k closest nodes it has not already queried".
    fn at_capacity(&self) -> bool {
        match self.state {
            State::Stalled => self.num_waiting >= self.config.num_results,
            State::Iterating { .. } => self.num_waiting >= self.config.parallelism,
            State::Finished => true
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Private state

/// Internal state of the iterator.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum State {
    /// The iterator is making progress by iterating towards `num_results` closest
    /// peers to the target with a maximum of `parallelism` peers for which the
    /// iterator is waiting for results at a time.
    ///
    /// > **Note**: When the iterator switches back to `Iterating` after being
    /// > `Stalled`, it may temporarily be waiting for more than `parallelism`
    /// > results from peers, with new peers only being considered once
    /// > the number pending results drops below `parallelism`.
    Iterating {
        /// The number of consecutive results that did not yield a peer closer
        /// to the target. When this number reaches `parallelism` and no new
        /// peer was discovered or at least `num_results` peers are known to
        /// the iterator, it is considered `Stalled`.
        no_progress: usize,
    },

    /// A iterator is stalled when it did not make progress after `parallelism`
    /// consecutive successful results (see `on_success`).
    ///
    /// While the iterator is stalled, the maximum allowed parallelism for pending
    /// results is increased to `num_results` in an attempt to finish the iterator.
    /// If the iterator can make progress again upon receiving the remaining
    /// results, it switches back to `Iterating`. Otherwise it will be finished.
    Stalled,

    /// The iterator is finished.
    ///
    /// A iterator finishes either when it has collected `num_results` results
    /// from the closest peers (not counting those that failed or are unresponsive)
    /// or because the iterator ran out of peers that have not yet delivered
    /// results (or failed).
    Finished
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct PathId(u8);

/// Representation of a peer in the context of a iterator.
#[derive(Debug, Clone)]
struct Peer {
    key: Key<PeerId>,
    state: PeerState
}

/// The state of a single `Peer`.
#[derive(Debug, Copy, Clone)]
enum PeerState {
    /// The peer has not yet been contacted.
    ///
    /// This is the starting state for every peer.
    NotContacted(Option<PathId>),

    /// The iterator is waiting for a result from the peer.
    Waiting((Instant, PathId)),

    /// A result was not delivered for the peer within the configured timeout.
    ///
    /// The peer is not taken into account for the termination conditions
    /// of the iterator until and unless it responds.
    Unresponsive(PathId),

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
    use multihash::Multihash;
    use rand::{Rng, thread_rng};
    use std::{iter, time::Duration};

    fn random_peers(n: usize) -> impl Iterator<Item = PeerId> + Clone {
        (0 .. n).map(|_| PeerId::random())
    }

    fn random_iter<G: Rng>(g: &mut G) -> ClosestPeersIter {
        let known_closest_peers = random_peers(g.gen_range(1, 60)).map(Key::from);
        let target = Key::from(Into::<Multihash>::into(PeerId::random()));
        let config = ClosestPeersIterConfig {
            parallelism: g.gen_range(1, 10),
            num_results: g.gen_range(1, 25),
            peer_timeout: Duration::from_secs(g.gen_range(10, 30)),
            disjoint_paths: unsafe { NonZeroU8::new_unchecked(g.gen_range(1, 10)) },
        };
        ClosestPeersIter::with_config(config, target, known_closest_peers)
    }

    fn sorted<T: AsRef<KeyBytes>>(target: &T, peers: &Vec<Key<PeerId>>) -> bool {
        peers.windows(2).all(|w| w[0].distance(&target) < w[1].distance(&target))
    }

    impl Arbitrary for ClosestPeersIter {
        fn arbitrary<G: Gen>(g: &mut G) -> ClosestPeersIter {
            random_iter(g)
        }
    }

    #[test]
    fn new_iter() {
        let iter = random_iter(&mut thread_rng());
        let target = iter.target.clone();

        let (keys, states): (Vec<_>, Vec<_>) = iter.closest_peers
            .values()
            .map(|e| (e.key.clone(), &e.state))
            .unzip();

        let none_contacted = states
            .iter()
            .all(|s| match s {
                PeerState::NotContacted(_) => true,
                _ => false
            });

        assert!(none_contacted,
            "Unexpected peer state in new iterator.");
        assert!(sorted(&target, &keys),
            "Closest peers in new iterator not sorted by distance to target.");
        assert_eq!(iter.num_waiting(), 0,
            "Unexpected peers in progress in new iterator.");
        assert_eq!(iter.into_result().count(), 0,
            "Unexpected closest peers in new iterator");
    }
    #[test]
    fn termination_and_parallelism() {
        fn prop(mut iter: ClosestPeersIter) {
            iter.config.disjoint_paths = unsafe { NonZeroU8::new_unchecked(1) };
            let now = Instant::now();
            let mut rng = thread_rng();

            let mut expected = iter.closest_peers
                .values()
                .map(|e| e.key.clone())
                .collect::<Vec<_>>();
            let num_known = expected.len();
            let max_parallelism = usize::min(iter.config.parallelism, num_known);

            let target = iter.target.clone();
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

                // Advance for maximum parallelism.
                for k in expected.iter() {
                    match iter.next(now) {
                        PeersIterState::Finished => break 'finished,
                        PeersIterState::Waiting(Some(p)) => assert_eq!(&*p, k.preimage()),
                        PeersIterState::Waiting(None) => panic!("Expected another peer."),
                        PeersIterState::WaitingAtCapacity => panic!("Unexpectedly reached capacity.")
                    }
                }
                let num_waiting = iter.num_waiting();
                assert_eq!(num_waiting, expected.len());

                // Check the bounded parallelism.
                if iter.at_capacity() {
                    assert_eq!(iter.next(now), PeersIterState::WaitingAtCapacity)
                }

                // Report results back to the iterator with a random number of "closer"
                // peers or an error, thus finishing the "in-flight requests".
                for (i, k) in expected.iter().enumerate() {
                    if rng.gen_bool(0.75) {
                        let num_closer = rng.gen_range(0, iter.config.num_results + 1);
                        let closer_peers = random_peers(num_closer).collect::<Vec<_>>();
                        remaining.extend(closer_peers.iter().cloned().map(Key::from));
                        iter.on_success(k.preimage(), closer_peers);
                    } else {
                        num_failures += 1;
                        iter.on_failure(k.preimage());
                    }
                    assert_eq!(iter.num_waiting(), num_waiting - (i + 1));
                }

                // Re-sort the remaining expected peers for the next "round".
                remaining.sort_by_key(|k| target.distance(&k));

                expected = remaining
            }

            // The iterator must be finished.
            assert_eq!(iter.next(now), PeersIterState::Finished);
            assert_eq!(iter.state, State::Finished);

            // Determine if all peers have been contacted by the iterator. This _must_ be
            // the case if the iterator finished with fewer than the requested number
            // of results.
            let all_contacted = iter.closest_peers.values().all(|e| match e.state {
                PeerState::NotContacted(_) | PeerState::Waiting { .. } => false,
                _ => true
            });

            let target = iter.target.clone();
            let num_results = iter.config.num_results;
            let result = iter.into_result();
            let closest = result.map(Key::from).collect::<Vec<_>>();

            assert!(sorted(&target, &closest));

            if closest.len() < num_results {
                // The iterator returned fewer results than requested. Therefore
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
    fn termination_and_parallelism_disjoint() {
        fn prop(mut iter: ClosestPeersIter) {
            let now = Instant::now();
            let mut rng = thread_rng();

            let mut not_yet_contacted = iter.closest_peers
                .values()
                .map(|e| e.key.clone())
                .collect::<Vec<_>>();

            let num_known = not_yet_contacted.len();

            let mut in_flight: Vec<Key<_>> = vec![];
            let mut num_failures = 0;

            'finished: loop {
                if not_yet_contacted.is_empty() && in_flight.is_empty() {
                    break;
                }

                loop {
                    if not_yet_contacted.is_empty() {
                        break;
                    }

                    match iter.next(now) {
                        PeersIterState::Finished => break 'finished,
                        PeersIterState::Waiting(Some(p)) => {
                            match not_yet_contacted.iter().position(|k| &*p == k.preimage()) {
                                Some(pos) => {
                                    in_flight.push(not_yet_contacted.remove(pos));
                                },
                                None => panic!("expected one of {:?} but got {:?}", not_yet_contacted, &*p),
                            }
                        },
                        PeersIterState::Waiting(None) => panic!("Expected another peer."),
                        PeersIterState::WaitingAtCapacity => {
                            break;
                        }
                    }
                }

                let num_waiting = iter.num_waiting();
                assert_eq!(num_waiting, in_flight.len());

                // Check the bounded parallelism.
                if iter.at_capacity() {
                    assert_eq!(iter.next(now), PeersIterState::WaitingAtCapacity)
                }

                // Report results back to the iterator with a random number of "closer"
                // peers or an error, thus finishing the "in-flight requests".
                for (i, k) in in_flight.drain(0..).enumerate() {
                    if rng.gen_bool(0.75) {
                        let num_closer = rng.gen_range(0, iter.config.num_results + 1);
                        let closer_peers = random_peers(num_closer).collect::<Vec<_>>();
                        not_yet_contacted.extend(closer_peers.iter().cloned().map(Key::from));
                        iter.on_success(k.preimage(), closer_peers);
                    } else {
                        num_failures += 1;
                        iter.on_failure(k.preimage());
                    }
                    assert_eq!(iter.num_waiting(), num_waiting - (i + 1));
                }

                assert!(in_flight.is_empty());
            }

            // The iterator must be finished.
            assert_eq!(iter.next(now), PeersIterState::Finished);
            assert_eq!(iter.state, State::Finished);

            // Determine if all peers have been contacted by the iterator. This _must_ be
            // the case if the iterator finished with fewer than the requested number
            // of results.
            let all_contacted = iter.closest_peers.values().all(|e| match e.state {
                PeerState::NotContacted(_) | PeerState::Waiting { .. } => false,
                _ => true
            });

            let target = iter.target.clone();
            let num_results = iter.config.num_results;
            let result = iter.into_result();
            let closest = result.map(Key::from).collect::<Vec<_>>();

            assert!(sorted(&target, &closest));

            if closest.len() < num_results {
                // The iterator returned fewer results than requested. Therefore
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
        fn prop(mut iter: ClosestPeersIter) -> bool {
            let now = Instant::now();
            let closer = random_peers(1).collect::<Vec<_>>();

            // A first peer reports a "closer" peer.
            let peer1 = match iter.next(now) {
                PeersIterState::Waiting(Some(p)) => p.into_owned(),
                _ => panic!("No peer.")
            };
            iter.on_success(&peer1, closer.clone());
            // Duplicate result from te same peer.
            iter.on_success(&peer1, closer.clone());

            // If there is a second peer, let it also report the same "closer" peer.
            match iter.next(now) {
                PeersIterState::Waiting(Some(p)) => {
                    let peer2 = p.into_owned();
                    iter.on_success(&peer2, closer.clone())
                }
                PeersIterState::Finished => {}
                _ => panic!("Unexpectedly iter state."),
            };

            // The "closer" peer must only be in the iterator once.
            let n = iter.closest_peers.values().filter(|e| e.key.preimage() == &closer[0]).count();
            assert_eq!(n, 1);

            true
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_) -> _)
    }

    #[test]
    fn timeout() {
        fn prop(mut iter: ClosestPeersIter) -> bool {
            let mut now = Instant::now();
            let peer = iter.closest_peers.values().next().unwrap().key.clone().into_preimage();

            // Poll the iterator for the first peer to be in progress.
            match iter.next(now) {
                PeersIterState::Waiting(Some(id)) => assert_eq!(&*id, &peer),
                _ => panic!()
            }

            // Artificially advance the clock.
            now = now + iter.config.peer_timeout;

            // Advancing the iterator again should mark the first peer as unresponsive.
            let _ = iter.next(now);
            match &iter.closest_peers.values().next().unwrap() {
                Peer { key, state: PeerState::Unresponsive(_) } => {
                    assert_eq!(key.preimage(), &peer);
                },
                Peer { state, .. } => panic!("Unexpected peer state: {:?}", state)
            }

            let finished = iter.finished();
            iter.on_success(&peer, iter::empty());
            let closest = iter.into_result().collect::<Vec<_>>();

            if finished {
                // Delivering results when the iterator already finished must have
                // no effect.
                assert_eq!(Vec::<PeerId>::new(), closest)
            } else {
                // Unresponsive peers can still deliver results while the iterator
                // is not finished.
                assert_eq!(vec![peer], closest)
            }
            true
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_) -> _)
    }

    #[test]
    fn s_kademlia_disjoint_paths() {
        let target: KeyBytes = Key::from(PeerId::random()).into();

        let mut pool = [0; 12].into_iter().map(|_| Key::from(PeerId::random())).collect::<Vec<_>>();
        pool.sort_unstable_by(|a, b| {
            target.distance(a).cmp(&target.distance(b))
        });
        let known_closest_peers = pool.split_off(pool.len() - 3);

        let mut config = ClosestPeersIterConfig::default();
        config.disjoint_paths = NonZeroU8::new(3).unwrap();

        let mut closest_peers_iter = ClosestPeersIter::with_config(
            config,
            target,
            known_closest_peers.clone(),
        );

        for i in 0..3 {
            assert_eq!(
                PeersIterState::Waiting(Some(Cow::Borrowed(known_closest_peers[i].preimage()))),
                closest_peers_iter.next(Instant::now()),
            );
        }

        // At capacity.
        assert_eq!(
            PeersIterState::WaitingAtCapacity,
            closest_peers_iter.next(Instant::now()),
        );

        let response_2 = pool.split_off(pool.len() - 3);
        let response_3 = pool.split_off(pool.len() - 3);
        // Keys are closer than any of the previous two responses from honest node 1 and 2.
        let malicious_response_1 = pool.split_off(pool.len() - 3);

        // Response from malicious peer 1.
        closest_peers_iter.on_success(
            known_closest_peers[0].preimage(),
            malicious_response_1.clone().into_iter().map(|k| k.preimage().clone()),
        );

        // Response from peer 2.
        closest_peers_iter.on_success(
            known_closest_peers[1].preimage(),
            response_2.clone().into_iter().map(|k| k.preimage().clone()),
        );

        // Response from peer 3.
        closest_peers_iter.on_success(
            known_closest_peers[2].preimage(),
            response_3.clone().into_iter().map(|k| k.preimage().clone()),
        );

        assert_eq!(
            PeersIterState::Waiting(Some(Cow::Borrowed(malicious_response_1[0].preimage()))),
            closest_peers_iter.next(Instant::now()),
        );

        assert_eq!(
            PeersIterState::Waiting(Some(Cow::Borrowed(response_2[0].preimage()))),
            closest_peers_iter.next(Instant::now()),
        );

        assert_eq!(
            PeersIterState::Waiting(Some(Cow::Borrowed(response_3[0].preimage()))),
            closest_peers_iter.next(Instant::now()),
        );

        // At capacity.
        assert_eq!(
            PeersIterState::WaitingAtCapacity,
            closest_peers_iter.next(Instant::now()),
        );
    }

    #[test]
    /// Kademlia's iterative query should proceed until the k closest nodes were queried
    /// successfully, or there are no further nodes to query that have not succeeded, failed or
    /// timed out.
    fn continue_until_k_closest_peers_succeed() {
        let now = Instant::now();
        let target: KeyBytes = Key::from(PeerId::random()).into();
        let default_num_results = ClosestPeersIterConfig::default().num_results;

        let mut pool = (0..default_num_results + 1).into_iter()
            .map(|_| Key::from(PeerId::random()))
            .collect::<Vec<_>>();

        pool.sort_unstable_by(|a, b| {
            target.distance(a).cmp(&target.distance(b))
        });

        let closest_peer = pool.remove(0);

        // Instantiate iterator with all but closest-peer.
        let mut closest_peers_iter = ClosestPeersIter::new(
            target,
            pool,
        );

        // Make all peers in iterator return successfully, with the last one returning the not yet
        // discovered closest-peer.
        for i in (0..default_num_results).into_iter() {
            match closest_peers_iter.next(now) {
                PeersIterState::Waiting(Some(peer_id)) => {
                    let peer_id = peer_id.into_owned();
                    closest_peers_iter.on_success(
                        &peer_id,
                        if i != default_num_results - 1 {
                            vec![]
                        } else {
                            vec![closest_peer.preimage().clone()]
                        }
                    );
                },
                PeersIterState::Waiting(None) => {
                    panic!("Expect iterator to have more peers to query.")
                }
                PeersIterState::WaitingAtCapacity => {
                    panic!("Expect iterator not to be at capacity.");
                },
                PeersIterState::Finished => {
                    panic!(
                        "Expect iterator not to finish before querying available closest peer or \
                         reaching the timeout.",
                    );
                }
            }
        }

        assert_eq!(
            PeersIterState::Waiting(Some(Cow::Borrowed(closest_peer.preimage()))),
            closest_peers_iter.next(now),
            "Expect iterator to return closest-peer. The iterator has {:?} succeeded peers, but \
             closest-peer is closer than any of them.",
            default_num_results,
        );

        assert_eq!(
            PeersIterState::Waiting(None),
            closest_peers_iter.next(now),
            "Expect iterator to wait for response from closest-peer."
        );

        assert_eq!(
            PeersIterState::Finished,
            closest_peers_iter.next(now + closest_peers_iter.config.peer_timeout),
            "Expect iterator to finish once query for closest-peer timed out and no other peers are \
             available."
        );
    }
}
