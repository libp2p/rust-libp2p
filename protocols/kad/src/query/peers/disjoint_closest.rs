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

    // TODO: Document that it needs to be equal or larger than parallelism?
    pub disjoint_paths: usize,

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
            disjoint_paths: 3,
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
        let iters = split_inputs_per_disjoint_path(&config, known_closest_peers)
            .into_iter()
            .map(|(config, peers)| ClosestPeersIter::with_config(config, target.clone(), peers))
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

/// Takes as input a set of resources (allowed parallelism, striven for number of results, set of
/// peers) and splits them equally (best-effort) into buckets, one for each disjoint path.
///
/// 'best-effort' as in no more than one apart. E.g. with 10 peers and 4 disjoint paths it would
/// return [3, 3, 2, 2].
//
// TODO: What if parallelism is smaller than disjoint_paths? In that case we would not explore as
// many disjoint paths as we could.
fn split_inputs_per_disjoint_path<I>(
    config: &DisjointClosestPeersIterConfig,
    peers: I,
) -> Vec<(ClosestPeersIterConfig, Vec<Key<PeerId>>)>
where
    I: IntoIterator<Item = Key<PeerId>>,
{
    let parallelism_per_iter = config.parallelism / config.disjoint_paths;
    let remaining_parallelism = config.parallelism % config.disjoint_paths;

    let num_results_per_iter = config.num_results / config.disjoint_paths;
    let remaining_num_results = config.num_results % config.disjoint_paths;

    let mut peers = peers.into_iter()
        // Each `[ClosestPeersIterConfig]` should be configured with ALPHA_VALUE
        // peers at initialization.
        .take(ALPHA_VALUE.get() * config.disjoint_paths)
        .collect::<Vec<Key<PeerId>>>();
    let peers_per_iter = peers.len() / config.disjoint_paths;
    let remaining_peers = peers.len() % config.disjoint_paths;

    (0..config.disjoint_paths).map(|i| {
        let parallelism = if i < remaining_parallelism {
            parallelism_per_iter + 1
        } else {
            parallelism_per_iter
        };

        let num_results = if i < remaining_num_results {
            num_results_per_iter + 1
        } else {
            num_results_per_iter
        };

        // TODO: Should we shuffle the peers beforehand? They might be sorted which will lead to
        // unfairness between iterators.
        let peers = if i < remaining_peers {
            peers.split_off(peers.len() - (peers_per_iter + 1))
        } else {
            peers.split_off(peers.len() - peers_per_iter)
        };

        (
            ClosestPeersIterConfig {
                parallelism,
                num_results,
                peer_timeout: config.peer_timeout,
            },
            peers,
        )
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;
    use rand::Rng;

    impl Arbitrary for DisjointClosestPeersIterConfig {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            DisjointClosestPeersIterConfig {
                parallelism: g.gen::<u16>() as usize,
                num_results: g.gen::<u16>() as usize,
                disjoint_paths: g.gen::<u16>() as usize,
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
    fn split_inputs_per_disjoint_path_quickcheck() {
        fn prop(config: DisjointClosestPeersIterConfig, peers: PeerVec) -> TestResult {
            if config.parallelism == 0
                || config.num_results == 0
                || config.disjoint_paths == 0
                || peers.0.len() == 0
            {
                return TestResult::discard();
            }

            let mut iters = split_inputs_per_disjoint_path(&config, peers.0.clone());

            // Ensure the sum of each resource of each disjoint path equals the allowed input.

            assert_eq!(
                config.parallelism,
                iters
                    .iter()
                    .fold(0, |acc, (config, _)| { acc + config.parallelism }),
            );

            assert_eq!(
                config.num_results,
                iters
                    .iter()
                    .fold(0, |acc, (config, _)| { acc + config.num_results }),
            );

            assert_eq!(
                peers.0.len(),
                iters
                    .iter()
                    .fold(0, |acc, (_, peers)| { acc + peers.len() }),
            );

            // Ensure 'best-effort' fairness, the highest and lowest are newer more than 1 apart.

            iters.sort_by(|(a_config, _), (b_config, _)| {
                a_config.parallelism.cmp(&b_config.parallelism)
            });
            assert!(iters[iters.len() - 1].0.parallelism - iters[0].0.parallelism <= 1);

            iters.sort_by(|(a_config, _), (b_config, _)| {
                a_config.num_results.cmp(&b_config.num_results)
            });
            assert!(iters[iters.len() - 1].0.num_results - iters[0].0.num_results <= 1);

            iters.sort_by(|(_, a_peers), (_, b_peers)| a_peers.len().cmp(&b_peers.len()));
            assert!(iters[iters.len() - 1].1.len() - iters[0].1.len() <= 1);

            TestResult::passed()
        }

        quickcheck(prop as fn(_, _) -> _)
    }

    #[test]
    fn s_kademlia_disjoint_paths() {
        let now = Instant::now();
        let target: KeyBytes = Key::from(PeerId::random()).into();

        let mut pool = [0; 12].into_iter()
            .map(|_| Key::from(PeerId::random()))
            .collect::<Vec<_>>();

        pool.sort_unstable_by(|a, b| {
            target.distance(a).cmp(&target.distance(b))
        });

        let known_closest_peers = pool.split_off(pool.len() - 3);

        let config = DisjointClosestPeersIterConfig {
            parallelism: 3,
            disjoint_paths: 3,
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
}
