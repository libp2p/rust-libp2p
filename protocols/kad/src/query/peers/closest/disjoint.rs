// Copyright 2020 Parity Technologies (UK) Ltd.
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
use crate::kbucket::{Key, KeyBytes};
use libp2p_core::PeerId;
use std::{collections::{HashMap, HashSet}, ops::{Add, Index, IndexMut}};
use wasm_timer::Instant;

/// Wraps around a set of [`ClosestPeersIter`], enforcing a disjoint discovery
/// path per configured parallelism according to the S/Kademlia paper.
pub struct ClosestDisjointPeersIter {
    config: ClosestPeersIterConfig,

    /// The set of wrapped [`ClosestPeersIter`].
    iters: Vec<ClosestPeersIter>,

    /// Mapping of contacted peers by their [`PeerId`] to [`PeerState`]
    /// containing the corresponding iterator indices as well as the response
    /// state.
    ///
    /// Used to track which iterator contacted which peer. See [`PeerState`]
    /// for details.
    contacted_peers: HashMap<PeerId, PeerState>,

    /// Index of the iterator last queried.
    last_queried: IteratorIndex,
}

impl ClosestDisjointPeersIter {
    /// Creates a new iterator with a default configuration.
    pub fn new<I>(target: KeyBytes, known_closest_peers: I) -> Self
    where
        I: IntoIterator<Item = Key<PeerId>>,
    {
        Self::with_config(
            ClosestPeersIterConfig::default(),
            target,
            known_closest_peers,
        )
    }

    /// Creates a new iterator with the given configuration.
    pub fn with_config<I, T>(
        config: ClosestPeersIterConfig,
        target: T,
        known_closest_peers: I,
    ) -> Self
    where
        I: IntoIterator<Item = Key<PeerId>>,
        T: Into<KeyBytes> + Clone,
    {
        let peers = known_closest_peers.into_iter().take(K_VALUE.get()).collect::<Vec<_>>();
        let iters = (0..config.parallelism.get())
            // NOTE: All [`ClosestPeersIter`] share the same set of peers at
            // initialization. The [`ClosestDisjointPeersIter.contacted_peers`]
            // mapping ensures that a successful response from a peer is only
            // ever passed to a single [`ClosestPeersIter`]. See
            // [`ClosestDisjointPeersIter::on_success`] for details.
            .map(|_| ClosestPeersIter::with_config(config.clone(), target.clone(), peers.clone()))
            .collect::<Vec<_>>();

        let iters_len = iters.len();

        ClosestDisjointPeersIter {
            config,
            iters,
            contacted_peers: HashMap::new(),
            // Wraps around, thus iterator 0 will be queried first.
            last_queried: (iters_len - 1).into(),
        }
    }

    pub fn on_failure(&mut self, peer: &PeerId) {
        // All peer failures are reported to all queries and thus to all peer
        // iterators. If this iterator never started a request to the given peer
        // ignore the failure.
        if let Some(PeerState{ initiated_by, additionally_awaited_by, response }) = self.contacted_peers.get_mut(peer) {
            *response = ResponseState::Failed;

            self.iters[*initiated_by].on_failure(peer);

            for i in additionally_awaited_by {
                self.iters[*i].on_failure(peer);
            }
        }
    }

    pub fn on_success<I>(&mut self, peer: &PeerId, closer_peers: I)
    where
        I: IntoIterator<Item = PeerId>,
    {
        if let Some(PeerState{ initiated_by, additionally_awaited_by, response }) = self.contacted_peers.get_mut(peer) {
            // Mark the response as succeeded for future iterators querying this
            // peer. There is no need to keep the `closer_peers` around, given
            // that they are only passed to the first iterator.
            *response = ResponseState::Succeeded;

            // Pass the new `closer_peers` to the iterator that first contacted
            // the peer.
            self.iters[*initiated_by].on_success(peer, closer_peers);

            for i in additionally_awaited_by {
                // Only report the success to all remaining not-first iterators.
                // Do not pass the `closer_peers` in order to uphold the
                // S/Kademlia disjoint paths guarantee.
                self.iters[*i].on_success(peer, std::iter::empty());
            }
        }
    }

    pub fn is_waiting(&self, peer: &PeerId) -> bool {
        if let Some(PeerState{ initiated_by, additionally_awaited_by, .. }) = self.contacted_peers.get(peer) {
            for i in std::iter::once(initiated_by).chain(additionally_awaited_by.iter()) {
                if self.iters[*i].is_waiting(peer) {
                    return true;
                }
            }
        }

        false
    }

    pub fn next(&mut self, now: Instant) -> PeersIterState {
        let mut state = None;

        // Order in which to query the iterators to ensure fairness. Make sure
        // to query the previously queried iterator last.
        let iter_order = {
            let mut all = (0..self.iters.len()).map(Into::into).collect::<Vec<_>>();
            let mut next_up = all.split_off((self.last_queried + 1).into());
            next_up.append(&mut all);
            next_up
        };

        for i in &mut iter_order.into_iter() {
            self.last_queried = i;
            let iter = &mut self.iters[i];

            loop {
                match iter.next(now) {
                    PeersIterState::Waiting(None) => {
                        match state {
                            Some(PeersIterState::Waiting(Some(_))) => {
                                // [`ClosestDisjointPeersIter::next`] returns immediately once a
                                // [`ClosestPeersIter`] yielded a peer. Thus this state is
                                // unreachable.
                                unreachable!();
                            },
                            Some(PeersIterState::Waiting(None)) => {}
                            Some(PeersIterState::WaitingAtCapacity) => {
                                // At least one ClosestPeersIter is no longer at capacity, thus the
                                // composite ClosestDisjointPeersIter is no longer at capacity.
                                state = Some(PeersIterState::Waiting(None))
                            }
                            Some(PeersIterState::Finished) => {
                                // `state` is never set to `Finished`.
                                unreachable!();
                            }
                            None => state = Some(PeersIterState::Waiting(None)),

                        };

                        break;
                    }
                    PeersIterState::Waiting(Some(peer)) => {
                        match self.contacted_peers.get_mut(&*peer) {
                            Some(PeerState{ additionally_awaited_by, response, .. }) => {
                                // Another iterator already contacted this peer.

                                let peer = peer.into_owned();
                                additionally_awaited_by.push(i);

                                match response {
                                    // The iterator will be notified later whether the given node
                                    // was successfully contacted or not. See
                                    // [`ClosestDisjointPeersIter::on_success`] for details.
                                    ResponseState::Waiting => {},
                                    ResponseState::Succeeded => {
                                        // Given that iterator was not the first to contact the peer
                                        // it will not be made aware of the closer peers discovered
                                        // to uphold the S/Kademlia disjoint paths guarantee. See
                                        // [`ClosestDisjointPeersIter::on_success`] for details.
                                        iter.on_success(&peer, std::iter::empty());
                                    },
                                    ResponseState::Failed => {
                                        iter.on_failure(&peer);
                                    },
                                }
                            },
                            None => {
                                // The iterator is the first to contact this peer.
                                self.contacted_peers.insert(
                                    peer.clone().into_owned(),
                                    PeerState::new(i),
                                );
                                return PeersIterState::Waiting(Some(Cow::Owned(peer.into_owned())));
                            },
                        }
                    }
                    PeersIterState::WaitingAtCapacity => {
                        match state {
                            Some(PeersIterState::Waiting(Some(_))) => {
                                // [`ClosestDisjointPeersIter::next`] returns immediately once a
                                // [`ClosestPeersIter`] yielded a peer. Thus this state is
                                // unreachable.
                                unreachable!();
                            },
                            Some(PeersIterState::Waiting(None)) => {}
                            Some(PeersIterState::WaitingAtCapacity) => {}
                            Some(PeersIterState::Finished) => {
                                // `state` is never set to `Finished`.
                                unreachable!();
                            },
                            None => state = Some(PeersIterState::WaitingAtCapacity),
                        };

                        break;
                    }
                    PeersIterState::Finished => break,
                }
            }
        }

        state.unwrap_or(PeersIterState::Finished)
    }

    pub fn finish(&mut self) {
        for iter in &mut self.iters {
            iter.finish()
        }
    }

    pub fn into_result(self) -> impl Iterator<Item = PeerId> {
        let mut result = HashSet::new();

        let mut iters = self.iters.into_iter().map(ClosestPeersIter::into_result).collect::<Vec<_>>();

        'outer: loop {
            let mut progress = false;

            for iter in iters.iter_mut() {
                if let Some(peer) = iter.next() {
                    progress = true;
                    result.insert(peer);
                    if result.len() == self.config.num_results.get() {
                        break 'outer;
                    }
                }
            }

            if !progress {
                break;
            }
        }

        result.into_iter()
    }
}

/// Index into the [`ClosestDisjointPeersIter`] `iters` vector.
#[derive(Copy, Clone)]
struct IteratorIndex(usize);

impl From<usize> for IteratorIndex {
    fn from(i: usize) -> Self {
        IteratorIndex(i)
    }
}

impl From<IteratorIndex> for usize {
    fn from(i: IteratorIndex) -> Self {
        i.0
    }
}

impl Add<usize> for IteratorIndex {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        (self.0 + rhs).into()
    }
}

impl Index<IteratorIndex> for Vec<ClosestPeersIter> {
    type Output = ClosestPeersIter;

    fn index(&self, index: IteratorIndex) -> &Self::Output {
        &self[index.0]
    }
}

impl IndexMut<IteratorIndex> for Vec<ClosestPeersIter> {
    fn index_mut(&mut self, index: IteratorIndex) -> &mut Self::Output {
        &mut self[index.0]
    }
}

/// State tracking the iterators that yielded (i.e. tried to contact) a peer. See
/// [`ClosestDisjointPeersIter::on_success`] for details.
struct PeerState {
    /// First iterator to yield the peer. Will be notified both of the outcome
    /// (success/failure) as well as the closer peers.
    initiated_by: IteratorIndex,
    /// Additional iterators only notified of the outcome (success/failure), not
    /// the closer peers, in order to uphold the S/Kademlia disjoint paths
    /// guarantee.
    additionally_awaited_by: Vec<IteratorIndex>,
    response: ResponseState,
}

impl PeerState {
    fn new(initiated_by: IteratorIndex) -> Self {
        PeerState {
            initiated_by,
            additionally_awaited_by: vec![],
            response: ResponseState::Waiting,
        }
    }
}

enum ResponseState {
    Waiting,
    Succeeded,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::K_VALUE;
    use quickcheck::*;
    use rand::{Rng, seq::SliceRandom};
    use std::collections::HashSet;

    #[derive(Debug, Clone)]
    struct Parallelism(NonZeroUsize);

    impl Arbitrary for Parallelism{
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            Parallelism(NonZeroUsize::new(g.gen_range(1, 10)).unwrap())
        }
    }

    #[derive(Debug, Clone)]
    struct NumResults(NonZeroUsize);

    impl Arbitrary for NumResults{
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            NumResults(NonZeroUsize::new(g.gen_range(1, K_VALUE.get())).unwrap())
        }
    }

    impl Arbitrary for ClosestPeersIterConfig {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            ClosestPeersIterConfig {
                parallelism: Parallelism::arbitrary(g).0,
                num_results: NumResults::arbitrary(g).0,
                peer_timeout: Duration::from_secs(1),
            }
        }
    }

    #[derive(Debug, Clone)]
    struct PeerVec(pub Vec<Key<PeerId>>);

    impl Arbitrary for PeerVec {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            PeerVec(
                (0..g.gen_range(1, 60))
                    .map(|_| PeerId::random())
                    .map(Key::from)
                    .collect(),
            )
        }
    }

    #[test]
    fn s_kademlia_disjoint_paths() {
        let now = Instant::now();
        let target: KeyBytes = Key::from(PeerId::random()).into();

        let mut pool = [0; 12].iter()
            .map(|_| Key::from(PeerId::random()))
            .collect::<Vec<_>>();

        pool.sort_unstable_by(|a, b| {
            target.distance(a).cmp(&target.distance(b))
        });

        let known_closest_peers = pool.split_off(pool.len() - 3);

        let config = ClosestPeersIterConfig {
            parallelism: NonZeroUsize::new(3).unwrap(),
            num_results: NonZeroUsize::new(3).unwrap(),
            ..ClosestPeersIterConfig::default()
        };

        let mut peers_iter = ClosestDisjointPeersIter::with_config(
            config.clone(),
            target,
            known_closest_peers.clone(),
        );

        //////////////////////////////////////////////////////////////////////////////
        // First round.

        for _ in 0..3 {
            if let PeersIterState::Waiting(Some(Cow::Owned(peer))) = peers_iter.next(now) {
                assert!(known_closest_peers.contains(&Key::from(peer)));
            } else {
                panic!("Expected iterator to return peer to query.");
            }
        }

        assert_eq!(
            PeersIterState::WaitingAtCapacity,
            peers_iter.next(now),
        );

        let response_2 = pool.split_off(pool.len() - 3);
        let response_3 = pool.split_off(pool.len() - 3);
        // Keys are closer than any of the previous two responses from honest node 1 and 2.
        let malicious_response_1 = pool.split_off(pool.len() - 3);

        // Response from malicious peer 1.
        peers_iter.on_success(
            known_closest_peers[0].preimage(),
            malicious_response_1.clone().into_iter().map(|k| k.preimage().clone()),
        );

        // Response from peer 2.
        peers_iter.on_success(
            known_closest_peers[1].preimage(),
            response_2.clone().into_iter().map(|k| k.preimage().clone()),
        );

        // Response from peer 3.
        peers_iter.on_success(
            known_closest_peers[2].preimage(),
            response_3.clone().into_iter().map(|k| k.preimage().clone()),
        );

        //////////////////////////////////////////////////////////////////////////////
        // Second round.

        let mut next_to_query = vec![];
        for _ in 0..3 {
            if let PeersIterState::Waiting(Some(Cow::Owned(peer))) = peers_iter.next(now) {
                next_to_query.push(peer)
            } else {
                panic!("Expected iterator to return peer to query.");
            }
        };

        // Expect a peer from each disjoint path.
        assert!(next_to_query.contains(malicious_response_1[0].preimage()));
        assert!(next_to_query.contains(response_2[0].preimage()));
        assert!(next_to_query.contains(response_3[0].preimage()));

        for peer in next_to_query {
            peers_iter.on_success(&peer, vec![]);
        }

        // Mark all remaining peers as succeeded.
        for _ in 0..6 {
            if let PeersIterState::Waiting(Some(Cow::Owned(peer))) = peers_iter.next(now) {
                peers_iter.on_success(&peer, vec![]);
            } else {
                panic!("Expected iterator to return peer to query.");
            }
        }

        assert_eq!(
            PeersIterState::Finished,
            peers_iter.next(now),
        );

        let final_peers: Vec<_> = peers_iter.into_result().collect();

        assert_eq!(config.num_results.get(), final_peers.len());

        // Expect final result to contain peer from each disjoint path, even though not all are
        // among the best ones.
        assert!(final_peers.contains(malicious_response_1[0].preimage()));
        assert!(final_peers.contains(response_2[0].preimage()));
        assert!(final_peers.contains(response_3[0].preimage()));
    }

    fn random_peers(n: usize) -> impl Iterator<Item = PeerId> + Clone {
        (0 .. n).map(|_| PeerId::random())
    }

    #[derive(Clone)]
    struct Graph(HashMap<PeerId, Peer>);

    impl std::fmt::Debug for Graph {
        fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
            fmt.debug_list().entries(self.0.iter().map(|(id, _)| id)).finish()
        }
    }

    impl Arbitrary for Graph {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let mut peer_ids = random_peers(g.gen_range(K_VALUE.get(), 200))
                .map(|peer_id| (peer_id.clone(), Key::from(peer_id)))
                .collect::<Vec<_>>();

            // Make each peer aware of its direct neighborhood.
            let mut peers = peer_ids.clone().into_iter()
                .map(|(peer_id, key)| {
                    peer_ids.sort_unstable_by(|(_, a), (_, b)| {
                        key.distance(a).cmp(&key.distance(b))
                    });

                    assert_eq!(peer_id, peer_ids[0].0);

                    let known_peers = peer_ids.iter()
                        // Skip itself.
                        .skip(1)
                        .take(K_VALUE.get())
                        .cloned()
                        .collect::<Vec<_>>();

                    (peer_id, Peer{ known_peers })
                })
                .collect::<HashMap<_, _>>();

            // Make each peer aware of a random set of other peers within the graph.
            for (peer_id, peer) in peers.iter_mut() {
                peer_ids.shuffle(g);

                let num_peers = g.gen_range(K_VALUE.get(), peer_ids.len() + 1);
                // let num_peers = peer_ids.len();
                let mut random_peer_ids = peer_ids.choose_multiple(g, num_peers)
                    // Make sure not to include itself.
                    .filter(|(id, _)| peer_id != id)
                    .cloned()
                    .collect::<Vec<_>>();

                peer.known_peers.append(&mut random_peer_ids);
                peer.known_peers = std::mem::replace(&mut peer.known_peers, vec![])
                    // Deduplicate peer ids.
                    .into_iter().collect::<HashSet<_>>().into_iter().collect();
            }

            Graph(peers)
        }
    }

    impl Graph {
        fn get_closest_peer(&self, target: &KeyBytes) -> PeerId {
            self.0.iter()
                .map(|(peer_id, _)| (target.distance(&Key::new(peer_id.clone())), peer_id))
                .fold(None, |acc, (distance_b, peer_id_b)| {
                    match acc {
                        None => Some((distance_b, peer_id_b)),
                        Some((distance_a, peer_id_a)) => if distance_a < distance_b {
                            Some((distance_a, peer_id_a))
                        } else {
                            Some((distance_b, peer_id_b))
                        }
                    }

                })
                .expect("Graph to have at least one peer.")
                .1.clone()
        }
    }

    #[derive(Debug, Clone)]
    struct Peer {
        known_peers: Vec<(PeerId, Key<PeerId>)>,
    }

    impl Peer {
        fn get_closest_peers(&mut self, target: &KeyBytes) -> Vec<PeerId> {
            self.known_peers.sort_unstable_by(|(_, a), (_, b)| {
                target.distance(a).cmp(&target.distance(b))
            });

            self.known_peers.iter().take(K_VALUE.get()).map(|(id, _)| id).cloned().collect()
        }
    }

    enum PeerIterator {
        Disjoint(ClosestDisjointPeersIter),
        Closest(ClosestPeersIter),
    }

    impl PeerIterator {
        fn next(&mut self, now: Instant) -> PeersIterState {
            match self {
                PeerIterator::Disjoint(iter) => iter.next(now),
                PeerIterator::Closest(iter) => iter.next(now),
            }
        }

        fn on_success(&mut self, peer: &PeerId, closer_peers: Vec<PeerId>) {
            match self {
                PeerIterator::Disjoint(iter) => iter.on_success(peer, closer_peers),
                PeerIterator::Closest(iter) => iter.on_success(peer, closer_peers),
            }
        }

        fn into_result(self) -> Vec<PeerId> {
            match self {
                PeerIterator::Disjoint(iter) => iter.into_result().collect(),
                PeerIterator::Closest(iter) => iter.into_result().collect(),
            }
        }
    }

    /// Ensure [`ClosestPeersIter`] and [`ClosestDisjointPeersIter`] yield same closest peers.
    #[test]
    fn closest_and_disjoint_closest_yield_same_result() {
        fn prop(graph: Graph, parallelism: Parallelism, num_results: NumResults) -> TestResult {
            if parallelism.0 > num_results.0 {
                return TestResult::discard();
            }

            let target: KeyBytes = Key::from(PeerId::random()).into();
            let closest_peer = graph.get_closest_peer(&target);

            let mut known_closest_peers = graph.0.iter()
                .take(K_VALUE.get())
                .map(|(key, _peers)| Key::new(key.clone()))
                .collect::<Vec<_>>();
            known_closest_peers.sort_unstable_by(|a, b| {
                target.distance(a).cmp(&target.distance(b))
            });

            let cfg = ClosestPeersIterConfig{
                parallelism: parallelism.0,
                num_results: num_results.0,
                ..ClosestPeersIterConfig::default()
            };

            let closest = drive_to_finish(
                PeerIterator::Closest(ClosestPeersIter::with_config(
                    cfg.clone(),
                    target.clone(),
                    known_closest_peers.clone(),
                )),
                graph.clone(),
                &target,
            );

            let disjoint = drive_to_finish(
                PeerIterator::Disjoint(ClosestDisjointPeersIter::with_config(
                    cfg,
                    target.clone(),
                    known_closest_peers.clone(),
                )),
                graph.clone(),
                &target,
            );

            assert!(
                closest.contains(&closest_peer),
                "Expected ClosestPeersIter to find closest peer.",
            );
            assert!(
                disjoint.contains(&closest_peer),
                "Expected ClosestDisjointPeersIter to find closest peer.",
            );

            assert_eq!(closest.len(), disjoint.len());

            if closest != disjoint {
                let closest_only = closest.difference(&disjoint).collect::<Vec<_>>();
                let disjoint_only = disjoint.difference(&closest).collect::<Vec<_>>();

                panic!(
                    "Expected both iterators to derive same peer set, but only `ClosestPeersIter` \
                     got {:?} and only `ClosestDisjointPeersIter` got {:?}.",
                    closest_only, disjoint_only,
                );
            };

            TestResult::passed()
        }

        fn drive_to_finish(
            mut iter: PeerIterator,
            mut graph: Graph,
            target: &KeyBytes,
        ) -> HashSet<PeerId> {
            let now = Instant::now();
            loop {
                match iter.next(now) {
                    PeersIterState::Waiting(Some(peer_id)) => {
                        let peer_id = peer_id.clone().into_owned();
                        let closest_peers = graph.0.get_mut(&peer_id)
                            .unwrap()
                            .get_closest_peers(&target);
                        iter.on_success(&peer_id, closest_peers);
                    } ,
                    PeersIterState::WaitingAtCapacity | PeersIterState::Waiting(None) =>
                        panic!("There is never more than one request in flight."),
                    PeersIterState::Finished => break,
                }
            }

            let mut result = iter.into_result().into_iter().map(Key::new).collect::<Vec<_>>();
            result.sort_unstable_by(|a, b| {
                target.distance(a).cmp(&target.distance(b))
            });
            result.into_iter().map(|k| k.into_preimage()).collect()
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_, _, _) -> _)
    }
}
