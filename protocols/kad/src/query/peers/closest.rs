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
use std::{time::Duration, iter::FromIterator};
use std::collections::btree_map::{BTreeMap, Entry};
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
}

impl Default for ClosestPeersIterConfig {
    fn default() -> Self {
        ClosestPeersIterConfig {
            parallelism: ALPHA_VALUE as usize,
            num_results: K_VALUE as usize,
            peer_timeout: Duration::from_secs(10),
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
                    let state = PeerState::NotContacted;
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
            num_waiting: 0
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

        // Mark the peer as succeeded.
        match self.closest_peers.entry(distance) {
            Entry::Vacant(..) => return,
            Entry::Occupied(mut e) => match e.get().state {
                PeerState::Waiting(..) => {
                    debug_assert!(self.num_waiting > 0);
                    self.num_waiting -= 1;
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::Unresponsive => {
                    e.get_mut().state = PeerState::Succeeded;
                }
                PeerState::NotContacted
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
            let peer = Peer { key, state: PeerState::NotContacted };
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
                PeerState::Unresponsive => {
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

        for peer in self.closest_peers.values_mut() {
            match peer.state {
                PeerState::Waiting(timeout) => {
                    if now >= timeout {
                        // Unresponsive peers no longer count towards the limit for the
                        // bounded parallelism, though they might still be ongoing and
                        // their results can still be delivered to the iterator.
                        debug_assert!(self.num_waiting > 0);
                        self.num_waiting -= 1;
                        peer.state = PeerState::Unresponsive
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

                PeerState::NotContacted =>
                    if !at_capacity {
                        let timeout = now + self.config.peer_timeout;
                        peer.state = PeerState::Waiting(timeout);
                        self.num_waiting += 1;
                        return PeersIterState::Waiting(Some(Cow::Borrowed(peer.key.preimage())))
                    } else {
                        return PeersIterState::WaitingAtCapacity
                    }

                PeerState::Unresponsive | PeerState::Failed => {
                    // Skip over unresponsive or failed peers.
                }
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
    NotContacted,

    /// The iterator is waiting for a result from the peer.
    Waiting(Instant),

    /// A result was not delivered for the peer within the configured timeout.
    ///
    /// The peer is not taken into account for the termination conditions
    /// of the iterator until and unless it responds.
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
                PeerState::NotContacted => true,
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
                PeerState::NotContacted | PeerState::Waiting { .. } => false,
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
                Peer { key, state: PeerState::Unresponsive } => {
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
}
