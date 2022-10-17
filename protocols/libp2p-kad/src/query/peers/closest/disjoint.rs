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
use instant::Instant;
use libp2p_core::PeerId;
use std::{
    collections::HashMap,
    iter::{Cycle, Map, Peekable},
    ops::{Index, IndexMut, Range},
};

/// Wraps around a set of [`ClosestPeersIter`], enforcing a disjoint discovery
/// path per configured parallelism according to the S/Kademlia paper.
pub struct ClosestDisjointPeersIter {
    config: ClosestPeersIterConfig,
    target: KeyBytes,

    /// The set of wrapped [`ClosestPeersIter`].
    iters: Vec<ClosestPeersIter>,
    /// Order in which to query the iterators ensuring fairness across
    /// [`ClosestPeersIter::next`] calls.
    iter_order: Cycle<Map<Range<usize>, fn(usize) -> IteratorIndex>>,

    /// Mapping of contacted peers by their [`PeerId`] to [`PeerState`]
    /// containing the corresponding iterator indices as well as the response
    /// state.
    ///
    /// Used to track which iterator contacted which peer. See [`PeerState`]
    /// for details.
    contacted_peers: HashMap<PeerId, PeerState>,
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
        let peers = known_closest_peers
            .into_iter()
            .take(K_VALUE.get())
            .collect::<Vec<_>>();
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
            target: target.into(),
            iters,
            iter_order: (0..iters_len)
                .map(IteratorIndex as fn(usize) -> IteratorIndex)
                .cycle(),
            contacted_peers: HashMap::new(),
        }
    }

    /// Callback for informing the iterator about a failed request to a peer.
    ///
    /// If the iterator is currently waiting for a result from `peer`,
    /// the iterator state is updated and `true` is returned. In that
    /// case, after calling this function, `next` should eventually be
    /// called again to obtain the new state of the iterator.
    ///
    /// If the iterator is finished, it is not currently waiting for a
    /// result from `peer`, or a result for `peer` has already been reported,
    /// calling this function has no effect and `false` is returned.
    pub fn on_failure(&mut self, peer: &PeerId) -> bool {
        let mut updated = false;

        if let Some(PeerState {
            initiated_by,
            response,
        }) = self.contacted_peers.get_mut(peer)
        {
            updated = self.iters[*initiated_by].on_failure(peer);

            if updated {
                *response = ResponseState::Failed;
            }

            for (i, iter) in &mut self.iters.iter_mut().enumerate() {
                if IteratorIndex(i) != *initiated_by {
                    // This iterator never triggered an actual request to the
                    // given peer - thus ignore the returned boolean.
                    iter.on_failure(peer);
                }
            }
        }

        updated
    }

    /// Callback for delivering the result of a successful request to a peer.
    ///
    /// Delivering results of requests back to the iterator allows the iterator
    /// to make progress. The iterator is said to make progress either when the
    /// given `closer_peers` contain a peer closer to the target than any peer
    /// seen so far, or when the iterator did not yet accumulate `num_results`
    /// closest peers and `closer_peers` contains a new peer, regardless of its
    /// distance to the target.
    ///
    /// If the iterator is currently waiting for a result from `peer`,
    /// the iterator state is updated and `true` is returned. In that
    /// case, after calling this function, `next` should eventually be
    /// called again to obtain the new state of the iterator.
    ///
    /// If the iterator is finished, it is not currently waiting for a
    /// result from `peer`, or a result for `peer` has already been reported,
    /// calling this function has no effect and `false` is returned.
    pub fn on_success<I>(&mut self, peer: &PeerId, closer_peers: I) -> bool
    where
        I: IntoIterator<Item = PeerId>,
    {
        let mut updated = false;

        if let Some(PeerState {
            initiated_by,
            response,
        }) = self.contacted_peers.get_mut(peer)
        {
            // Pass the new `closer_peers` to the iterator that first yielded
            // the peer.
            updated = self.iters[*initiated_by].on_success(peer, closer_peers);

            if updated {
                // Mark the response as succeeded for future iterators yielding
                // this peer. There is no need to keep the `closer_peers`
                // around, given that they are only passed to the first
                // iterator.
                *response = ResponseState::Succeeded;
            }

            for (i, iter) in &mut self.iters.iter_mut().enumerate() {
                if IteratorIndex(i) != *initiated_by {
                    // Only report the success to all remaining not-first
                    // iterators. Do not pass the `closer_peers` in order to
                    // uphold the S/Kademlia disjoint paths guarantee.
                    //
                    // This iterator never triggered an actual request to the
                    // given peer - thus ignore the returned boolean.
                    iter.on_success(peer, std::iter::empty());
                }
            }
        }

        updated
    }

    pub fn is_waiting(&self, peer: &PeerId) -> bool {
        self.iters.iter().any(|i| i.is_waiting(peer))
    }

    pub fn next(&mut self, now: Instant) -> PeersIterState<'_> {
        let mut state = None;

        // Ensure querying each iterator at most once.
        for _ in 0..self.iters.len() {
            let i = self.iter_order.next().expect("Cycle never ends.");
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
                            }
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
                            Some(PeerState { response, .. }) => {
                                // Another iterator already contacted this peer.
                                let peer = peer.into_owned();

                                match response {
                                    // The iterator will be notified later whether the given node
                                    // was successfully contacted or not. See
                                    // [`ClosestDisjointPeersIter::on_success`] for details.
                                    ResponseState::Waiting => {}
                                    ResponseState::Succeeded => {
                                        // Given that iterator was not the first to contact the peer
                                        // it will not be made aware of the closer peers discovered
                                        // to uphold the S/Kademlia disjoint paths guarantee. See
                                        // [`ClosestDisjointPeersIter::on_success`] for details.
                                        iter.on_success(&peer, std::iter::empty());
                                    }
                                    ResponseState::Failed => {
                                        iter.on_failure(&peer);
                                    }
                                }
                            }
                            None => {
                                // The iterator is the first to contact this peer.
                                self.contacted_peers
                                    .insert(peer.clone().into_owned(), PeerState::new(i));
                                return PeersIterState::Waiting(Some(Cow::Owned(
                                    peer.into_owned(),
                                )));
                            }
                        }
                    }
                    PeersIterState::WaitingAtCapacity => {
                        match state {
                            Some(PeersIterState::Waiting(Some(_))) => {
                                // [`ClosestDisjointPeersIter::next`] returns immediately once a
                                // [`ClosestPeersIter`] yielded a peer. Thus this state is
                                // unreachable.
                                unreachable!();
                            }
                            Some(PeersIterState::Waiting(None)) => {}
                            Some(PeersIterState::WaitingAtCapacity) => {}
                            Some(PeersIterState::Finished) => {
                                // `state` is never set to `Finished`.
                                unreachable!();
                            }
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

    /// Finishes all paths containing one of the given peers.
    ///
    /// See [`crate::query::Query::try_finish`] for details.
    pub fn finish_paths<'a, I>(&mut self, peers: I) -> bool
    where
        I: IntoIterator<Item = &'a PeerId>,
    {
        for peer in peers {
            if let Some(PeerState { initiated_by, .. }) = self.contacted_peers.get_mut(peer) {
                self.iters[*initiated_by].finish();
            }
        }

        self.is_finished()
    }

    /// Immediately transitions the iterator to [`PeersIterState::Finished`].
    pub fn finish(&mut self) {
        for iter in &mut self.iters {
            iter.finish();
        }
    }

    /// Checks whether the iterator has finished.
    pub fn is_finished(&self) -> bool {
        self.iters.iter().all(|i| i.is_finished())
    }

    /// Note: In the case of no adversarial peers or connectivity issues along
    ///       any path, all paths return the same result, deduplicated through
    ///       the `ResultIter`, thus overall `into_result` returns
    ///       `num_results`. In the case of adversarial peers or connectivity
    ///       issues `ClosestDisjointPeersIter` tries to return the
    ///       `num_results` closest benign peers, but as it can not
    ///       differentiate benign from faulty paths it as well returns faulty
    ///       peers and thus overall returns more than `num_results` peers.
    pub fn into_result(self) -> impl Iterator<Item = PeerId> {
        let result_per_path = self
            .iters
            .into_iter()
            .map(|iter| iter.into_result().map(Key::from));

        ResultIter::new(self.target, result_per_path).map(Key::into_preimage)
    }
}

/// Index into the [`ClosestDisjointPeersIter`] `iters` vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IteratorIndex(usize);

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

/// State tracking the iterator that yielded (i.e. tried to contact) a peer. See
/// [`ClosestDisjointPeersIter::on_success`] for details.
#[derive(Debug, PartialEq, Eq)]
struct PeerState {
    /// First iterator to yield the peer. Will be notified both of the outcome
    /// (success/failure) as well as the closer peers.
    initiated_by: IteratorIndex,
    /// Keeping track of the response state. In case other iterators later on
    /// yield the same peer, they can be notified of the response outcome.
    response: ResponseState,
}

impl PeerState {
    fn new(initiated_by: IteratorIndex) -> Self {
        PeerState {
            initiated_by,
            response: ResponseState::Waiting,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ResponseState {
    Waiting,
    Succeeded,
    Failed,
}

/// Iterator combining the result of multiple [`ClosestPeersIter`] into a single
/// deduplicated ordered iterator.
//
// Note: This operates under the assumption that `I` is ordered.
#[derive(Clone, Debug)]
struct ResultIter<I>
where
    I: Iterator<Item = Key<PeerId>>,
{
    target: KeyBytes,
    iters: Vec<Peekable<I>>,
}

impl<I: Iterator<Item = Key<PeerId>>> ResultIter<I> {
    fn new(target: KeyBytes, iters: impl Iterator<Item = I>) -> Self {
        ResultIter {
            target,
            iters: iters.map(Iterator::peekable).collect(),
        }
    }
}

impl<I: Iterator<Item = Key<PeerId>>> Iterator for ResultIter<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let target = &self.target;

        self.iters
            .iter_mut()
            // Find the iterator with the next closest peer.
            .fold(Option::<&mut Peekable<_>>::None, |iter_a, iter_b| {
                let iter_a = match iter_a {
                    Some(iter_a) => iter_a,
                    None => return Some(iter_b),
                };

                match (iter_a.peek(), iter_b.peek()) {
                    (Some(next_a), Some(next_b)) => {
                        if next_a == next_b {
                            // Remove from one for deduplication.
                            iter_b.next();
                            return Some(iter_a);
                        }

                        if target.distance(next_a) < target.distance(next_b) {
                            Some(iter_a)
                        } else {
                            Some(iter_b)
                        }
                    }
                    (Some(_), None) => Some(iter_a),
                    (None, Some(_)) => Some(iter_b),
                    (None, None) => None,
                }
            })
            // Pop off the next closest peer from that iterator.
            .and_then(Iterator::next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::K_VALUE;
    use libp2p_core::multihash::{Code, Multihash};
    use quickcheck::*;
    use std::collections::HashSet;
    use std::iter;

    impl Arbitrary for ResultIter<std::vec::IntoIter<Key<PeerId>>> {
        fn arbitrary(g: &mut Gen) -> Self {
            let target = Target::arbitrary(g).0;
            let num_closest_iters = g.gen_range(0..20 + 1);
            let peers = random_peers(g.gen_range(0..20 * num_closest_iters + 1), g);

            let iters = (0..num_closest_iters).map(|_| {
                let num_peers = g.gen_range(0..20 + 1);
                let mut peers = g
                    .choose_multiple(&peers, num_peers)
                    .cloned()
                    .map(Key::from)
                    .collect::<Vec<_>>();

                peers.sort_unstable_by_key(|a| target.distance(a));

                peers.into_iter()
            });

            ResultIter::new(target.clone(), iters)
        }

        fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
            let peers = self
                .iters
                .clone()
                .into_iter()
                .flatten()
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();

            let iters = self
                .iters
                .clone()
                .into_iter()
                .map(|iter| iter.collect::<Vec<_>>())
                .collect();

            Box::new(ResultIterShrinker {
                target: self.target.clone(),
                peers,
                iters,
            })
        }
    }

    struct ResultIterShrinker {
        target: KeyBytes,
        peers: Vec<Key<PeerId>>,
        iters: Vec<Vec<Key<PeerId>>>,
    }

    impl Iterator for ResultIterShrinker {
        type Item = ResultIter<std::vec::IntoIter<Key<PeerId>>>;

        /// Return an iterator of [`ResultIter`]s with each of them missing a
        /// different peer from the original set.
        fn next(&mut self) -> Option<Self::Item> {
            // The peer that should not be included.
            let peer = self.peers.pop()?;

            let iters = self.iters.clone().into_iter().filter_map(|mut iter| {
                iter.retain(|p| p != &peer);
                if iter.is_empty() {
                    return None;
                }
                Some(iter.into_iter())
            });

            Some(ResultIter::new(self.target.clone(), iters))
        }
    }

    #[derive(Clone, Debug)]
    struct ArbitraryPeerId(PeerId);

    impl Arbitrary for ArbitraryPeerId {
        fn arbitrary(g: &mut Gen) -> ArbitraryPeerId {
            let hash: [u8; 32] = core::array::from_fn(|_| u8::arbitrary(g));
            let peer_id =
                PeerId::from_multihash(Multihash::wrap(Code::Sha2_256.into(), &hash).unwrap())
                    .unwrap();
            ArbitraryPeerId(peer_id)
        }
    }

    #[derive(Clone, Debug)]
    struct Target(KeyBytes);

    impl Arbitrary for Target {
        fn arbitrary(g: &mut Gen) -> Self {
            let peer_id = ArbitraryPeerId::arbitrary(g).0;
            Target(Key::from(peer_id).into())
        }
    }

    fn random_peers(n: usize, g: &mut Gen) -> Vec<PeerId> {
        (0..n).map(|_| ArbitraryPeerId::arbitrary(g).0).collect()
    }

    #[test]
    fn result_iter_returns_deduplicated_ordered_peer_id_stream() {
        fn prop(result_iter: ResultIter<std::vec::IntoIter<Key<PeerId>>>) {
            let expected = {
                let mut deduplicated = result_iter
                    .clone()
                    .iters
                    .into_iter()
                    .flatten()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .map(Key::from)
                    .collect::<Vec<_>>();

                deduplicated.sort_unstable_by(|a, b| {
                    result_iter
                        .target
                        .distance(a)
                        .cmp(&result_iter.target.distance(b))
                });

                deduplicated
            };

            assert_eq!(expected, result_iter.collect::<Vec<_>>());
        }

        QuickCheck::new().quickcheck(prop as fn(_))
    }

    #[derive(Debug, Clone)]
    struct Parallelism(NonZeroUsize);

    impl Arbitrary for Parallelism {
        fn arbitrary(g: &mut Gen) -> Self {
            Parallelism(NonZeroUsize::new(g.gen_range(1..10)).unwrap())
        }
    }

    #[derive(Debug, Clone)]
    struct NumResults(NonZeroUsize);

    impl Arbitrary for NumResults {
        fn arbitrary(g: &mut Gen) -> Self {
            NumResults(NonZeroUsize::new(g.gen_range(1..K_VALUE.get())).unwrap())
        }
    }

    impl Arbitrary for ClosestPeersIterConfig {
        fn arbitrary(g: &mut Gen) -> Self {
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
        fn arbitrary(g: &mut Gen) -> Self {
            PeerVec(
                (0..g.gen_range(1..60u8))
                    .map(|_| ArbitraryPeerId::arbitrary(g).0)
                    .map(Key::from)
                    .collect(),
            )
        }
    }

    #[test]
    fn s_kademlia_disjoint_paths() {
        let now = Instant::now();
        let target: KeyBytes = Key::from(PeerId::random()).into();

        let mut pool = [0; 12]
            .iter()
            .map(|_| Key::from(PeerId::random()))
            .collect::<Vec<_>>();

        pool.sort_unstable_by_key(|a| target.distance(a));

        let known_closest_peers = pool.split_off(pool.len() - 3);

        let config = ClosestPeersIterConfig {
            parallelism: NonZeroUsize::new(3).unwrap(),
            num_results: NonZeroUsize::new(3).unwrap(),
            ..ClosestPeersIterConfig::default()
        };

        let mut peers_iter =
            ClosestDisjointPeersIter::with_config(config, target, known_closest_peers.clone());

        ////////////////////////////////////////////////////////////////////////
        // First round.

        for _ in 0..3 {
            if let PeersIterState::Waiting(Some(Cow::Owned(peer))) = peers_iter.next(now) {
                assert!(known_closest_peers.contains(&Key::from(peer)));
            } else {
                panic!("Expected iterator to return peer to query.");
            }
        }

        assert_eq!(PeersIterState::WaitingAtCapacity, peers_iter.next(now),);

        let response_2 = pool.split_off(pool.len() - 3);
        let response_3 = pool.split_off(pool.len() - 3);
        // Keys are closer than any of the previous two responses from honest
        // node 1 and 2.
        let malicious_response_1 = pool.split_off(pool.len() - 3);

        // Response from malicious peer 1.
        peers_iter.on_success(
            known_closest_peers[0].preimage(),
            malicious_response_1
                .clone()
                .into_iter()
                .map(|k| *k.preimage()),
        );

        // Response from peer 2.
        peers_iter.on_success(
            known_closest_peers[1].preimage(),
            response_2.clone().into_iter().map(|k| *k.preimage()),
        );

        // Response from peer 3.
        peers_iter.on_success(
            known_closest_peers[2].preimage(),
            response_3.clone().into_iter().map(|k| *k.preimage()),
        );

        ////////////////////////////////////////////////////////////////////////
        // Second round.

        let mut next_to_query = vec![];
        for _ in 0..3 {
            if let PeersIterState::Waiting(Some(Cow::Owned(peer))) = peers_iter.next(now) {
                next_to_query.push(peer)
            } else {
                panic!("Expected iterator to return peer to query.");
            }
        }

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

        assert_eq!(PeersIterState::Finished, peers_iter.next(now),);

        let final_peers: Vec<_> = peers_iter.into_result().collect();

        // Expect final result to contain peer from each disjoint path, even
        // though not all are among the best ones.
        assert!(final_peers.contains(malicious_response_1[0].preimage()));
        assert!(final_peers.contains(response_2[0].preimage()));
        assert!(final_peers.contains(response_3[0].preimage()));
    }

    #[derive(Clone)]
    struct Graph(HashMap<PeerId, Peer>);

    impl std::fmt::Debug for Graph {
        fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            fmt.debug_list()
                .entries(self.0.iter().map(|(id, _)| id))
                .finish()
        }
    }

    impl Arbitrary for Graph {
        fn arbitrary(g: &mut Gen) -> Self {
            let mut peer_ids = random_peers(g.gen_range(K_VALUE.get()..200), g)
                .into_iter()
                .map(|peer_id| (peer_id, Key::from(peer_id)))
                .collect::<Vec<_>>();

            // Make each peer aware of its direct neighborhood.
            let mut peers = peer_ids
                .clone()
                .into_iter()
                .map(|(peer_id, key)| {
                    peer_ids
                        .sort_unstable_by(|(_, a), (_, b)| key.distance(a).cmp(&key.distance(b)));

                    assert_eq!(peer_id, peer_ids[0].0);

                    let known_peers = peer_ids
                        .iter()
                        // Skip itself.
                        .skip(1)
                        .take(K_VALUE.get())
                        .cloned()
                        .collect::<Vec<_>>();

                    (peer_id, Peer { known_peers })
                })
                .collect::<HashMap<_, _>>();

            // Make each peer aware of a random set of other peers within the graph.
            for (peer_id, peer) in peers.iter_mut() {
                g.shuffle(&mut peer_ids);

                let num_peers = g.gen_range(K_VALUE.get()..peer_ids.len() + 1);
                let mut random_peer_ids = g
                    .choose_multiple(&peer_ids, num_peers)
                    // Make sure not to include itself.
                    .filter(|(id, _)| peer_id != id)
                    .cloned()
                    .collect::<Vec<_>>();

                peer.known_peers.append(&mut random_peer_ids);
                peer.known_peers = std::mem::take(&mut peer.known_peers)
                    // Deduplicate peer ids.
                    .into_iter()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect();
            }

            Graph(peers)
        }
    }

    impl Graph {
        fn get_closest_peer(&self, target: &KeyBytes) -> PeerId {
            *self
                .0
                .iter()
                .map(|(peer_id, _)| (target.distance(&Key::from(*peer_id)), peer_id))
                .fold(None, |acc, (distance_b, peer_id_b)| match acc {
                    None => Some((distance_b, peer_id_b)),
                    Some((distance_a, peer_id_a)) => {
                        if distance_a < distance_b {
                            Some((distance_a, peer_id_a))
                        } else {
                            Some((distance_b, peer_id_b))
                        }
                    }
                })
                .expect("Graph to have at least one peer.")
                .1
        }
    }

    #[derive(Debug, Clone)]
    struct Peer {
        known_peers: Vec<(PeerId, Key<PeerId>)>,
    }

    impl Peer {
        fn get_closest_peers(&mut self, target: &KeyBytes) -> Vec<PeerId> {
            self.known_peers
                .sort_unstable_by(|(_, a), (_, b)| target.distance(a).cmp(&target.distance(b)));

            self.known_peers
                .iter()
                .take(K_VALUE.get())
                .map(|(id, _)| id)
                .cloned()
                .collect()
        }
    }

    enum PeerIterator {
        Disjoint(ClosestDisjointPeersIter),
        Closest(ClosestPeersIter),
    }

    impl PeerIterator {
        fn next(&mut self, now: Instant) -> PeersIterState<'_> {
            match self {
                PeerIterator::Disjoint(iter) => iter.next(now),
                PeerIterator::Closest(iter) => iter.next(now),
            }
        }

        fn on_success(&mut self, peer: &PeerId, closer_peers: Vec<PeerId>) {
            match self {
                PeerIterator::Disjoint(iter) => iter.on_success(peer, closer_peers),
                PeerIterator::Closest(iter) => iter.on_success(peer, closer_peers),
            };
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
        fn prop(
            target: Target,
            graph: Graph,
            parallelism: Parallelism,
            num_results: NumResults,
        ) -> TestResult {
            if parallelism.0 > num_results.0 {
                return TestResult::discard();
            }

            let target: KeyBytes = target.0;
            let closest_peer = graph.get_closest_peer(&target);

            let mut known_closest_peers = graph
                .0
                .iter()
                .take(K_VALUE.get())
                .map(|(key, _peers)| Key::from(*key))
                .collect::<Vec<_>>();
            known_closest_peers.sort_unstable_by_key(|a| target.distance(a));

            let cfg = ClosestPeersIterConfig {
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
                graph,
                &target,
            );

            assert!(
                closest.contains(&closest_peer),
                "Expected `ClosestPeersIter` to find closest peer.",
            );
            assert!(
                disjoint.contains(&closest_peer),
                "Expected `ClosestDisjointPeersIter` to find closest peer.",
            );

            assert!(
                closest.len() == num_results.0.get(),
                "Expected `ClosestPeersIter` to find `num_results` closest \
                 peers."
            );
            assert!(
                disjoint.len() >= num_results.0.get(),
                "Expected `ClosestDisjointPeersIter` to find at least \
                 `num_results` closest peers."
            );

            if closest.len() > disjoint.len() {
                let closest_only = closest.difference(&disjoint).collect::<Vec<_>>();

                panic!(
                    "Expected `ClosestDisjointPeersIter` to find all peers \
                     found by `ClosestPeersIter`, but it did not find {:?}.",
                    closest_only,
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
                        let closest_peers =
                            graph.0.get_mut(&peer_id).unwrap().get_closest_peers(target);
                        iter.on_success(&peer_id, closest_peers);
                    }
                    PeersIterState::WaitingAtCapacity | PeersIterState::Waiting(None) => {
                        panic!("There is never more than one request in flight.")
                    }
                    PeersIterState::Finished => break,
                }
            }

            let mut result = iter
                .into_result()
                .into_iter()
                .map(Key::from)
                .collect::<Vec<_>>();
            result.sort_unstable_by_key(|a| target.distance(a));
            result.into_iter().map(|k| k.into_preimage()).collect()
        }

        QuickCheck::new()
            .tests(10)
            .quickcheck(prop as fn(_, _, _, _) -> _)
    }

    #[test]
    fn failure_can_not_overwrite_previous_success() {
        let now = Instant::now();
        let peer = PeerId::random();
        let mut iter = ClosestDisjointPeersIter::new(
            Key::from(PeerId::random()).into(),
            iter::once(Key::from(peer)),
        );

        assert!(matches!(iter.next(now), PeersIterState::Waiting(Some(_))));

        // Expect peer to be marked as succeeded.
        assert!(iter.on_success(&peer, iter::empty()));
        assert_eq!(
            iter.contacted_peers.get(&peer),
            Some(&PeerState {
                initiated_by: IteratorIndex(0),
                response: ResponseState::Succeeded,
            })
        );

        // Expect peer to stay marked as succeeded.
        assert!(!iter.on_failure(&peer));
        assert_eq!(
            iter.contacted_peers.get(&peer),
            Some(&PeerState {
                initiated_by: IteratorIndex(0),
                response: ResponseState::Succeeded,
            })
        );
    }
}
