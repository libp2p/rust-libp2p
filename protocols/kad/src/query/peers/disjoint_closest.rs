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

    let mut peers = peers.into_iter().collect::<Vec<Key<PeerId>>>();
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
}
