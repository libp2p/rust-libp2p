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
use crate::query::peers::closest::{ClosestPeersIter, ClosestPeersIterConfig};
use crate::{ALPHA_VALUE, K_VALUE};
use libp2p_core::PeerId;
use std::{collections::HashMap, time::Duration};
use wasm_timer::Instant;

// TODO: This duplicates a lot of documentation (See ClosestPeersIterConfig).
#[derive(Debug, Clone)]
pub struct DisjointClosestPeersIterConfig {
    /// Allowed level of parallelism.
    ///
    /// The `α` parameter in the Kademlia paper. The maximum number of peers that
    /// the iterator is allowed to wait for in parallel while iterating towards the closest
    /// nodes to a target. Defaults to `ALPHA_VALUE`.
    pub parallelism: usize,

    // TODO: Document that the number of disjoint paths is equal to the amount of parallelism.
    pub use_disjoint_paths: bool,

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

impl Default for DisjointClosestPeersIterConfig {
    fn default() -> Self {
        DisjointClosestPeersIterConfig {
            parallelism: ALPHA_VALUE.get(),
            use_disjoint_paths: false,
            num_results: K_VALUE.get(),
            peer_timeout: Duration::from_secs(10),
        }
    }
}

/// Wraps around a set of `ClosestPeersIter`, enforcing the amount of configured disjoint paths
/// according to the S/Kademlia paper.
pub struct DisjointClosestPeersIter {
    iters: Vec<ClosestPeersIter>,
    /// Mapping of yielded peers to iterator that yielded them.
    ///
    /// More specifically index into the `DisjointClosestPeersIter::iters` vector. On the one hand
    /// this is used to link responses from remote peers back to the corresponding iterator, on the
    /// other hand it is used to track which peers have been contacted in the past.
    yielded_peers: HashMap<PeerId, usize>,
}

impl DisjointClosestPeersIter {
    /// Creates a new iterator with a default configuration.
    pub fn new<I>(target: KeyBytes, known_closest_peers: I) -> Self
    where
        I: IntoIterator<Item = Key<PeerId>>,
    {
        Self::with_config(
            DisjointClosestPeersIterConfig::default(),
            target,
            known_closest_peers,
        )
    }

    /// Creates a new iterator with the given configuration.
    pub fn with_config<I, T>(
        config: DisjointClosestPeersIterConfig,
        target: T,
        known_closest_peers: I,
    ) -> Self
    where
        I: IntoIterator<Item = Key<PeerId>>,
        T: Into<KeyBytes> + Clone,
    {
        // TODO: Document that this basically makes the disjoint path iterator a no-op shallow wrapper.
        if !config.use_disjoint_paths {
            return DisjointClosestPeersIter {
                iters: vec![ClosestPeersIter::with_config(
                    ClosestPeersIterConfig {
                        parallelism: config.parallelism,
                        num_results: config.num_results,
                        peer_timeout: config.peer_timeout,
                    },
                    target,
                    known_closest_peers,
                )],
                yielded_peers: HashMap::new(),
            }
        }

        // TODO: Don't collect them all (all k-buckets), but only parts.
        let peers = known_closest_peers.into_iter().collect::<Vec<_>>();
        let iters = split_num_results_per_disjoint_path(&config)
            .into_iter()
            // NOTE: All [`ClosestPeersIter`] share the same set of peers at initialization. The
            // [`DisjointClosestPeersIter`] ensures a peer is only ever queried by a single
            // [`ClosestPeersIter`].
            .map(|config| ClosestPeersIter::with_config(config, target.clone(), peers.clone()))
            .collect();

        DisjointClosestPeersIter {
            iters,
            yielded_peers: HashMap::new(),
        }
    }

    pub fn on_failure(&mut self, peer: &PeerId) {
        self.iters[self.yielded_peers[peer]].on_failure(peer);
    }

    pub fn on_success<I>(&mut self, peer: &PeerId, closer_peers: I)
    where
        I: IntoIterator<Item = PeerId>,
    {
        self.iters[self.yielded_peers[peer]].on_success(peer, closer_peers);
    }

    pub fn is_waiting(&self, peer: &PeerId) -> bool {
        self.iters[self.yielded_peers[peer]].is_waiting(peer)
    }

    pub fn next(&mut self, now: Instant) -> PeersIterState {
        let mut state = PeersIterState::Finished;

        // TODO: Iterating through all iterators in the same order might be unfair.
        for (i, iter) in &mut self.iters.iter_mut().enumerate() {
            loop {
                match iter.next(now) {
                    PeersIterState::Waiting(None) => {
                        match state {
                            PeersIterState::Waiting(None) => {}
                            PeersIterState::WaitingAtCapacity => {
                                state = PeersIterState::Waiting(None)
                            }
                            PeersIterState::Finished => state = PeersIterState::Waiting(None),
                            // TODO: Document.
                            _ => unreachable!(),
                        };

                        break;
                    }
                    PeersIterState::Waiting(Some(peer)) => {
                        // TODO: Hack to get around the borrow checker. Can we do better?
                        let peer = peer.clone().into_owned();

                        if self.yielded_peers.contains_key(&peer) {
                            // Another iterator already returned this peer. S/Kademlia requires each
                            // peer to be only used on one path. Marking it as failed for this
                            // iterator, asking it to return another peer in the next loop
                            // iteration.
                            iter.on_failure(&peer);
                        } else {
                            self.yielded_peers.insert(peer.clone(), i);
                            return PeersIterState::Waiting(Some(Cow::Owned(peer)));
                        }
                    }
                    PeersIterState::WaitingAtCapacity => {
                        match state {
                            PeersIterState::Waiting(None) => {}
                            PeersIterState::WaitingAtCapacity => {}
                            PeersIterState::Finished => state = PeersIterState::WaitingAtCapacity,
                            // TODO: Document.
                            _ => unreachable!(),
                        };

                        break;
                    }
                    PeersIterState::Finished => break,
                }
            }
        }

        state
    }

    pub fn finish(&mut self) {
        for iter in &mut self.iters {
            iter.finish()
        }
    }

    // TODO: Collects all Iterators into a Vec and again returns an Iterator. Can we do better?
    pub fn into_result(self) -> impl Iterator<Item = PeerId> {
        self.iters
            .into_iter()
            .fold(vec![], |mut acc, iter| {
                acc.extend(iter.into_result());
                acc
            })
            .into_iter()
    }
}

/// Takes as input a [`DisjointClosestPeersIterConfig`] splits `num_results` for each disjoint path
/// (`== parallelism`) equally (best-effort) into buckets, one for each disjoint path returning a
/// `ClosestPeersIterConfig` for each disjoint path.
///
/// 'best-effort' as in no more than one apart. E.g. with 10 overall num_result and 4 disjoint paths
/// it would return [3, 3, 2, 2].
fn split_num_results_per_disjoint_path(
    config: &DisjointClosestPeersIterConfig,
) -> Vec<ClosestPeersIterConfig> {
    // Note: The number of parallelism is equal to the number of disjoint paths.
    let num_results_per_iter = config.num_results / config.parallelism;
    let remaining_num_results = config.num_results % config.parallelism;

    (0..config.parallelism).map(|i| {
        let num_results = if i < remaining_num_results {
            num_results_per_iter + 1
        } else {
            num_results_per_iter
        };

            ClosestPeersIterConfig {
                parallelism: 1,
                num_results,
                peer_timeout: config.peer_timeout,
            }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;
    use rand::{Rng, seq::SliceRandom};
    use std::collections::HashSet;

    impl Arbitrary for DisjointClosestPeersIterConfig {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            DisjointClosestPeersIterConfig {
                parallelism: g.gen::<u16>() as usize,
                num_results: g.gen::<u16>() as usize,
                use_disjoint_paths: g.gen::<bool>(),
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
    fn split_num_results_per_disjoint_path_quickcheck() {
        fn prop(config: DisjointClosestPeersIterConfig) -> TestResult {
            if config.parallelism == 0 || config.num_results == 0
            {
                return TestResult::discard();
            }

            let mut iters = split_num_results_per_disjoint_path(&config);

            // Ensure the sum of all disjoint paths equals the allowed input.

            assert_eq!(
                config.num_results,
                iters
                    .iter()
                    .fold(0, |acc, config| { acc + config.num_results }),
            );

            // Ensure 'best-effort' fairness, the highest and lowest are newer more than 1 apart.

            iters.sort_by(|a_config, b_config| {
                a_config.num_results.cmp(&b_config.num_results)
            });
            assert!(iters[iters.len() - 1].num_results - iters[0].num_results <= 1);

            TestResult::passed()
        }

        quickcheck(prop as fn(_) -> _)
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

        let config = DisjointClosestPeersIterConfig {
            parallelism: 3,
            use_disjoint_paths: true,
            num_results: 3,
            ..DisjointClosestPeersIterConfig::default()
        };

        let mut peers_iter = DisjointClosestPeersIter::with_config(
            config,
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

        assert_eq!(
            PeersIterState::Finished,
            peers_iter.next(now),
        );

        let final_peers: Vec<_> = peers_iter.into_result().collect();

        // Expect final result to contain peer from each disjoint path, even though not all are
        // among the best ones.
        assert!(final_peers.contains(malicious_response_1[0].preimage()));
        assert!(final_peers.contains(response_2[0].preimage()));
        assert!(final_peers.contains(response_3[0].preimage()));
    }

    fn random_peers(n: usize) -> impl Iterator<Item = PeerId> + Clone {
        (0 .. n).map(|_| PeerId::random())
    }

    #[derive(Debug, Clone)]
    struct Graph(HashMap<PeerId, Peer>);

    impl Arbitrary for Graph {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let mut peers = HashMap::new();
            let mut peer_ids = random_peers(g.gen_range(K_VALUE.get(), 500))
                .collect::<Vec<_>>();

            for peer_id in peer_ids.clone() {
                peer_ids.shuffle(g);

                peers.insert(peer_id, Peer{
                    known_peers: peer_ids[0..g.gen_range(K_VALUE.get(), peer_ids.len())].to_vec(),
                });
            }

            Graph(peers)
        }
    }

    #[derive(Debug, Clone)]
    struct Peer {
        known_peers: Vec<PeerId>,
    }

    #[derive(Debug, Clone)]
    struct Parallelism(usize);

    impl Arbitrary for Parallelism{
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            Parallelism(g.gen_range(1, 10))
        }
    }

    #[derive(Debug, Clone)]
    struct NumResults(usize);

    impl Arbitrary for NumResults{
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            NumResults(g.gen_range(1, K_VALUE.get()))
        }
    }

    enum PeerIterator {
        Disjoint(DisjointClosestPeersIter),
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

    /// Ensure [`ClosestPeersIter`] and [`DisjointClosestPeersIter`] yield similar closest peers.
    ///
    // NOTE: One can not ensure both iterators yield the *same* result. While [`ClosestPeersIter`]
    // always yields the closest peers, [`DisjointClosestPeersIter`] might not. Imagine a node on
    // path x yielding the 22 absolute closest peers, not returned by any other node. In addition
    // imagine only 10 results are allowed per path. Path x will choose the 10 closest out of those
    // 22 and drop the remaining 12, thus the overall [`DisjointClosestPeersIter`] will not yield
    // the absolute closest peers combining all paths.
    #[test]
    fn closest_and_disjoint_closest_yield_similar_result() {
        fn prop(graph: Graph, parallelism: Parallelism, num_results: NumResults) {
            let target: KeyBytes = Key::from(PeerId::random()).into();

            let mut known_closest_peers = graph.0.iter().take(parallelism.0 * ALPHA_VALUE.get()).map(|(key, _peers)| Key::new(key.clone())).collect::<Vec<_>>();
            known_closest_peers.sort_unstable_by(|a, b| {
                target.distance(a).cmp(&target.distance(b))
            });

            let closest = drive_to_finish(PeerIterator::Closest(ClosestPeersIter::with_config(
                ClosestPeersIterConfig{
                    parallelism: parallelism.0,
                    num_results: num_results.0,
                    ..ClosestPeersIterConfig::default()
                },
                target.clone(),
                known_closest_peers.clone(),
            )), &graph, &target);

            let disjoint = drive_to_finish(PeerIterator::Disjoint(DisjointClosestPeersIter::with_config(
                DisjointClosestPeersIterConfig{
                    parallelism: parallelism.0,
                    use_disjoint_paths: true,
                    num_results: num_results.0,
                    ..DisjointClosestPeersIterConfig::default()
                },
                target.clone(),
                known_closest_peers.clone(),
            )),&graph, &target);

            assert_eq!(
                closest.len(), disjoint.len()
            );

            if closest != disjoint {
                let closest_only = closest.difference(&disjoint).collect::<Vec<_>>();
                let disjoint_only = disjoint.difference(&closest).collect::<Vec<_>>();
                if closest_only.len() > num_results.0 / 2 {
                    panic!(
                        "Expected both iterators to derive same peer set or be no more than \
                         `(num_results / 2)` apart, but only `ClosestPeersIter` got {:?} and only \
                         `DisjointClosestPeersIter` got {:?}.",
                        closest_only, disjoint_only,
                    );
                }
            };
        }

        fn drive_to_finish(
            mut iter: PeerIterator,
            graph: &Graph,
            target: &KeyBytes,
        ) -> HashSet<PeerId> {
            let now = Instant::now();
            loop {
                match iter.next(now) {
                    PeersIterState::Waiting(Some(peer_id)) => {
                        let peer_id = peer_id.clone().into_owned();
                        iter.on_success(&peer_id, graph.0.get(&peer_id).unwrap().known_peers.clone())
                    } ,
                    PeersIterState::WaitingAtCapacity | PeersIterState::Waiting(None) => panic!("There are never more than one requests in flight."),
                    PeersIterState::Finished => break,
                }
            }

            let mut result = iter.into_result().into_iter().map(Key::new).collect::<Vec<_>>();
            result.sort_unstable_by(|a, b| {
                target.distance(a).cmp(&target.distance(b))
            });
            result.into_iter().map(|k| k.into_preimage()).collect()
        }

        QuickCheck::new().tests(10).quickcheck(prop as fn(_, _, _))
    }
}
