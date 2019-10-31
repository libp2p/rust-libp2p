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

#![cfg(test)]

use super::*;

use crate::K_VALUE;
use crate::kbucket::Distance;
use crate::record::store::MemoryStore;
use futures::future;
use libp2p_core::{
    PeerId,
    Transport,
    identity,
    transport::{MemoryTransport, boxed::Boxed},
    nodes::Substream,
    multiaddr::{Protocol, multiaddr},
    muxing::StreamMuxerBox,
    upgrade
};
use libp2p_secio::SecioConfig;
use libp2p_swarm::Swarm;
use libp2p_yamux as yamux;
use quickcheck::*;
use rand::{Rng, random, thread_rng};
use std::{collections::{HashSet, HashMap}, io, num::NonZeroUsize, u64};
use tokio::runtime::current_thread;
use multihash::{Multihash, Hash::SHA2256};

type TestSwarm = Swarm<
    Boxed<(PeerId, StreamMuxerBox), io::Error>,
    Kademlia<Substream<StreamMuxerBox>, MemoryStore>
>;

/// Builds swarms, each listening on a port. Does *not* connect the nodes together.
fn build_nodes(num: usize) -> (u64, Vec<TestSwarm>) {
    build_nodes_with_config(num, Default::default())
}

/// Builds swarms, each listening on a port. Does *not* connect the nodes together.
fn build_nodes_with_config(num: usize, cfg: KademliaConfig) -> (u64, Vec<TestSwarm>) {
    let port_base = 1 + random::<u64>() % (u64::MAX - num as u64);
    let mut result: Vec<Swarm<_, _>> = Vec::with_capacity(num);

    for _ in 0 .. num {
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = MemoryTransport::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(SecioConfig::new(local_key))
            .multiplex(yamux::Config::default())
            .map(|(p, m), _| (p, StreamMuxerBox::new(m)))
            .map_err(|e| panic!("Failed to create transport: {:?}", e))
            .boxed();

        let local_id = local_public_key.clone().into_peer_id();
        let store = MemoryStore::new(local_id.clone());
        let behaviour = Kademlia::with_config(local_id.clone(), store, cfg.clone());
        result.push(Swarm::new(transport, behaviour, local_id));
    }

    for (i, s) in result.iter_mut().enumerate() {
        Swarm::listen_on(s, Protocol::Memory(port_base + i as u64).into()).unwrap();
    }

    (port_base, result)
}

fn build_connected_nodes(total: usize, step: usize) -> (Vec<PeerId>, Vec<TestSwarm>) {
    build_connected_nodes_with_config(total, step, Default::default())
}

fn build_connected_nodes_with_config(total: usize, step: usize, cfg: KademliaConfig)
    -> (Vec<PeerId>, Vec<TestSwarm>)
{
    let (port_base, mut swarms) = build_nodes_with_config(total, cfg);
    let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

    let mut i = 0;
    for (j, peer) in swarm_ids.iter().enumerate().skip(1) {
        if i < swarm_ids.len() {
            swarms[i].add_address(&peer, Protocol::Memory(port_base + j as u64).into());
        }
        if j % step == 0 {
            i += step;
        }
    }

    (swarm_ids, swarms)
}

#[test]
fn bootstrap() {
    fn run(rng: &mut impl Rng) {
        let num_total = rng.gen_range(2, 20);
        let num_group = rng.gen_range(1, num_total);
        let (swarm_ids, mut swarms) = build_connected_nodes(num_total, num_group);

        swarms[0].bootstrap();

        // Expected known peers
        let expected_known = swarm_ids.iter().skip(1).cloned().collect::<HashSet<_>>();

        // Run test
        current_thread::run(
            future::poll_fn(move || {
                for (i, swarm) in swarms.iter_mut().enumerate() {
                    loop {
                        match swarm.poll().unwrap() {
                            Async::Ready(Some(KademliaEvent::BootstrapResult(Ok(ok)))) => {
                                assert_eq!(i, 0);
                                assert_eq!(ok.peer, swarm_ids[0]);
                                let known = swarm.kbuckets.iter()
                                    .map(|e| e.node.key.preimage().clone())
                                    .collect::<HashSet<_>>();
                                assert_eq!(expected_known, known);
                                return Ok(Async::Ready(()));
                            }
                            Async::Ready(_) => (),
                            Async::NotReady => break,
                        }
                    }
                }
                Ok(Async::NotReady)
            }))
    }

    let mut rng = thread_rng();
    for _ in 0 .. 10 {
        run(&mut rng)
    }
}

#[test]
fn query_iter() {
    fn distances<K>(key: &kbucket::Key<K>, peers: Vec<PeerId>) -> Vec<Distance> {
        peers.into_iter()
            .map(kbucket::Key::from)
            .map(|k| k.distance(key))
            .collect()
    }

    fn run(rng: &mut impl Rng) {
        let num_total = rng.gen_range(2, 20);
        let (swarm_ids, mut swarms) = build_connected_nodes(num_total, 1);

        // Ask the first peer in the list to search a random peer. The search should
        // propagate forwards through the list of peers.
        let search_target = PeerId::random();
        let search_target_key = kbucket::Key::new(search_target.clone());
        swarms[0].get_closest_peers(search_target.clone());

        // Set up expectations.
        let expected_swarm_id = swarm_ids[0].clone();
        let expected_peer_ids: Vec<_> = swarm_ids.iter().skip(1).cloned().collect();
        let mut expected_distances = distances(&search_target_key, expected_peer_ids.clone());
        expected_distances.sort();

        // Run test
        current_thread::run(
            future::poll_fn(move || {
                for (i, swarm) in swarms.iter_mut().enumerate() {
                    loop {
                        match swarm.poll().unwrap() {
                            Async::Ready(Some(KademliaEvent::GetClosestPeersResult(Ok(ok)))) => {
                                assert_eq!(&ok.key[..], search_target.as_bytes());
                                assert_eq!(swarm_ids[i], expected_swarm_id);
                                assert_eq!(swarm.queries.size(), 0);
                                assert!(expected_peer_ids.iter().all(|p| ok.peers.contains(p)));
                                let key = kbucket::Key::new(ok.key);
                                assert_eq!(expected_distances, distances(&key, ok.peers));
                                return Ok(Async::Ready(()));
                            }
                            Async::Ready(_) => (),
                            Async::NotReady => break,
                        }
                    }
                }
                Ok(Async::NotReady)
            }))
    }

    let mut rng = thread_rng();
    for _ in 0 .. 10 {
        run(&mut rng)
    }
}

#[test]
fn unresponsive_not_returned_direct() {
    // Build one node. It contains fake addresses to non-existing nodes. We ask it to find a
    // random peer. We make sure that no fake address is returned.

    let (_, mut swarms) = build_nodes(1);

    // Add fake addresses.
    for _ in 0 .. 10 {
        swarms[0].add_address(&PeerId::random(), Protocol::Udp(10u16).into());
    }

    // Ask first to search a random value.
    let search_target = PeerId::random();
    swarms[0].get_closest_peers(search_target.clone());

    current_thread::run(
        future::poll_fn(move || {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaEvent::GetClosestPeersResult(Ok(ok)))) => {
                            assert_eq!(&ok.key[..], search_target.as_bytes());
                            assert_eq!(ok.peers.len(), 0);
                            return Ok(Async::Ready(()));
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
}

#[test]
fn unresponsive_not_returned_indirect() {
    // Build two nodes. Node #2 knows about node #1. Node #1 contains fake addresses to
    // non-existing nodes. We ask node #2 to find a random peer. We make sure that no fake address
    // is returned.

    let (port_base, mut swarms) = build_nodes(2);

    // Add fake addresses to first.
    let first_peer_id = Swarm::local_peer_id(&swarms[0]).clone();
    for _ in 0 .. 10 {
        swarms[0].add_address(&PeerId::random(), multiaddr![Udp(10u16)]);
    }

    // Connect second to first.
    swarms[1].add_address(&first_peer_id, Protocol::Memory(port_base).into());

    // Ask second to search a random value.
    let search_target = PeerId::random();
    swarms[1].get_closest_peers(search_target.clone());

    current_thread::run(
        future::poll_fn(move || {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaEvent::GetClosestPeersResult(Ok(ok)))) => {
                            assert_eq!(&ok.key[..], search_target.as_bytes());
                            assert_eq!(ok.peers.len(), 1);
                            assert_eq!(ok.peers[0], first_peer_id);
                            return Ok(Async::Ready(()));
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
}

#[test]
fn get_record_not_found() {
    let (port_base, mut swarms) = build_nodes(3);

    let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

    swarms[0].add_address(&swarm_ids[1], Protocol::Memory(port_base + 1).into());
    swarms[1].add_address(&swarm_ids[2], Protocol::Memory(port_base + 2).into());

    let target_key = record::Key::from(Multihash::random(SHA2256));
    swarms[0].get_record(&target_key, Quorum::One);

    current_thread::run(
        future::poll_fn(move || {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaEvent::GetRecordResult(Err(e)))) => {
                            if let GetRecordError::NotFound { key, closest_peers, } = e {
                                assert_eq!(key, target_key);
                                assert_eq!(closest_peers.len(), 2);
                                assert!(closest_peers.contains(&swarm_ids[1]));
                                assert!(closest_peers.contains(&swarm_ids[2]));
                                return Ok(Async::Ready(()));
                            } else {
                                panic!("Unexpected error result: {:?}", e);
                            }
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
}

#[test]
fn put_record() {
    fn prop(replication_factor: usize, records: Vec<Record>) {
        let replication_factor = NonZeroUsize::new(replication_factor % (K_VALUE.get() / 2) + 1).unwrap();
        let num_total = replication_factor.get() * 2;
        let num_group = replication_factor.get();

        let mut config = KademliaConfig::default();
        config.set_replication_factor(replication_factor);
        let (swarm_ids, mut swarms) = build_connected_nodes_with_config(num_total, num_group, config);

        let records = records.into_iter()
            .take(num_total)
            .map(|mut r| {
                // We don't want records to expire prematurely, as they would
                // be removed from storage and no longer replicated, but we still
                // want to check that an explicitly set expiration is preserved.
                r.expires = r.expires.map(|t| t + Duration::from_secs(60));
                (r.key.clone(), r)
            })
            .collect::<HashMap<_,_>>();

        for r in records.values() {
            swarms[0].put_record(r.clone(), Quorum::All);
        }

        // Each test run republishes all records once.
        let mut republished = false;
        // The accumulated results for one round of publishing.
        let mut results = Vec::new();

        current_thread::run(
            future::poll_fn(move || loop {
                // Poll all swarms until they are "NotReady".
                for swarm in &mut swarms {
                    loop {
                        match swarm.poll().unwrap() {
                            Async::Ready(Some(KademliaEvent::PutRecordResult(res))) |
                            Async::Ready(Some(KademliaEvent::RepublishRecordResult(res))) => {
                                match res {
                                    Err(e) => panic!(e),
                                    Ok(ok) => {
                                        assert!(records.contains_key(&ok.key));
                                        let record = swarm.store.get(&ok.key).unwrap();
                                        results.push(record.into_owned());
                                    }
                                }
                            }
                            Async::Ready(_) => (),
                            Async::NotReady => break,
                        }
                    }
                }

                // All swarms are NotReady and not enough results have been collected
                // so far, thus wait to be polled again for further progress.
                if results.len() != records.len() {
                    return Ok(Async::NotReady)
                }

                // Consume the results, checking that each record was replicated
                // correctly to the closest peers to the key.
                while let Some(r) = results.pop() {
                    let expected = records.get(&r.key).unwrap();

                    assert_eq!(r.key, expected.key);
                    assert_eq!(r.value, expected.value);
                    assert_eq!(r.expires, expected.expires);
                    assert_eq!(r.publisher.as_ref(), Some(&swarm_ids[0]));

                    let key = kbucket::Key::new(r.key.clone());
                    let mut expected = swarm_ids.clone().split_off(1);
                    expected.sort_by(|id1, id2|
                        kbucket::Key::new(id1).distance(&key).cmp(
                            &kbucket::Key::new(id2).distance(&key)));

                    let expected = expected
                        .into_iter()
                        .take(replication_factor.get())
                        .collect::<HashSet<_>>();

                    let actual = swarms.iter().enumerate().skip(1)
                        .filter_map(|(i, s)|
                            if s.store.get(key.preimage()).is_some() {
                                Some(swarm_ids[i].clone())
                            } else {
                                None
                            })
                        .collect::<HashSet<_>>();

                    assert_eq!(actual.len(), replication_factor.get());
                    assert_eq!(actual, expected);
                }

                if republished {
                    assert_eq!(swarms[0].store.records().count(), records.len());
                    assert_eq!(swarms[0].queries.size(), 0);
                    for k in records.keys() {
                        swarms[0].store.remove(&k);
                    }
                    assert_eq!(swarms[0].store.records().count(), 0);
                    // All records have been republished, thus the test is complete.
                    return Ok(Async::Ready(()));
                }

                // Tell the replication job to republish asap.
                swarms[0].put_record_job.as_mut().unwrap().asap(true);
                republished = true;
            })
        )
    }

    QuickCheck::new().tests(3).quickcheck(prop as fn(_,_))
}

#[test]
fn get_value() {
    let (port_base, mut swarms) = build_nodes(3);

    let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

    swarms[0].add_address(&swarm_ids[1], Protocol::Memory(port_base + 1).into());
    swarms[1].add_address(&swarm_ids[2], Protocol::Memory(port_base + 2).into());

    let record = Record::new(Multihash::random(SHA2256), vec![4,5,6]);

    swarms[1].store.put(record.clone()).unwrap();
    swarms[0].get_record(&record.key, Quorum::One);

    current_thread::run(
        future::poll_fn(move || {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaEvent::GetRecordResult(Ok(ok)))) => {
                            assert_eq!(ok.records.len(), 1);
                            assert_eq!(ok.records.first(), Some(&record));
                            return Ok(Async::Ready(()));
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
}

#[test]
fn get_value_many() {
    // TODO: Randomise
    let num_nodes = 12;
    let (_, mut swarms) = build_connected_nodes(num_nodes, num_nodes);
    let num_results = 10;

    let record = Record::new(Multihash::random(SHA2256), vec![4,5,6]);

    for i in 0 .. num_nodes {
        swarms[i].store.put(record.clone()).unwrap();
    }

    let quorum = Quorum::N(NonZeroUsize::new(num_results).unwrap());
    swarms[0].get_record(&record.key, quorum);

    current_thread::run(
        future::poll_fn(move || {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaEvent::GetRecordResult(Ok(ok)))) => {
                            assert_eq!(ok.records.len(), num_results);
                            assert_eq!(ok.records.first(), Some(&record));
                            return Ok(Async::Ready(()));
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }
            Ok(Async::NotReady)
        }))
}

#[test]
fn add_provider() {
    fn prop(replication_factor: usize, keys: Vec<record::Key>) {
        let replication_factor = NonZeroUsize::new(replication_factor % (K_VALUE.get() / 2) + 1).unwrap();
        let num_total = replication_factor.get() * 2;
        let num_group = replication_factor.get();

        let mut config = KademliaConfig::default();
        config.set_replication_factor(replication_factor);

        let (swarm_ids, mut swarms) = build_connected_nodes_with_config(num_total, num_group, config);

        let keys: HashSet<_> = keys.into_iter().take(num_total).collect();

        // Each test run publishes all records twice.
        let mut published = false;
        let mut republished = false;
        // The accumulated results for one round of publishing.
        let mut results = Vec::new();

        // Initiate the first round of publishing.
        for k in &keys {
            swarms[0].start_providing(k.clone());
        }

        current_thread::run(
            future::poll_fn(move || loop {
                // Poll all swarms until they are "NotReady".
                for swarm in &mut swarms {
                    loop {
                        match swarm.poll().unwrap() {
                            Async::Ready(Some(KademliaEvent::StartProvidingResult(res))) |
                            Async::Ready(Some(KademliaEvent::RepublishProviderResult(res))) => {
                                match res {
                                    Err(e) => panic!(e),
                                    Ok(ok) => {
                                        assert!(keys.contains(&ok.key));
                                        results.push(ok.key);
                                    }
                                }
                            }
                            Async::Ready(_) => (),
                            Async::NotReady => break,
                        }
                    }
                }

                if results.len() == keys.len() {
                    // All requests have been sent for one round of publishing.
                    published = true
                }

                if !published {
                    // Still waiting for all requests to be sent for one round
                    // of publishing.
                    return Ok(Async::NotReady)
                }

                // A round of publishing is complete. Consume the results, checking that
                // each key was published to the `replication_factor` closest peers.
                while let Some(key) = results.pop() {
                    // Collect the nodes that have a provider record for `key`.
                    let actual = swarms.iter().enumerate().skip(1)
                        .filter_map(|(i, s)|
                            if s.store.providers(&key).len() == 1 {
                                Some(swarm_ids[i].clone())
                            } else {
                                None
                            })
                        .collect::<HashSet<_>>();

                    if actual.len() != replication_factor.get() {
                        // Still waiting for some nodes to process the request.
                        results.push(key);
                        return Ok(Async::NotReady)
                    }

                    let mut expected = swarm_ids.clone().split_off(1);
                    let kbucket_key = kbucket::Key::new(key);
                    expected.sort_by(|id1, id2|
                        kbucket::Key::new(id1).distance(&kbucket_key).cmp(
                            &kbucket::Key::new(id2).distance(&kbucket_key)));

                    let expected = expected
                        .into_iter()
                        .take(replication_factor.get())
                        .collect::<HashSet<_>>();

                    assert_eq!(actual, expected);
                }

                // One round of publishing is complete.
                assert!(results.is_empty());
                for s in &swarms {
                    assert_eq!(s.queries.size(), 0);
                }

                if republished {
                    assert_eq!(swarms[0].store.provided().count(), keys.len());
                    for k in &keys {
                        swarms[0].stop_providing(&k);
                    }
                    assert_eq!(swarms[0].store.provided().count(), 0);
                    // All records have been republished, thus the test is complete.
                    return Ok(Async::Ready(()));
                }

                // Initiate the second round of publishing by telling the
                // periodic provider job to run asap.
                swarms[0].add_provider_job.as_mut().unwrap().asap();
                published = false;
                republished = true;
            })
        )
    }

    QuickCheck::new().tests(3).quickcheck(prop as fn(_,_))
}

/// User code should be able to start queries beyond the internal
/// query limit for background jobs. Originally this even produced an
/// arithmetic overflow, see https://github.com/libp2p/rust-libp2p/issues/1290.
#[test]
fn exceed_jobs_max_queries() {
    let (_, mut swarms) = build_nodes(1);
    let num = JOBS_MAX_QUERIES + 1;
    for _ in 0 .. num {
        swarms[0].bootstrap();
    }

    assert_eq!(swarms[0].queries.size(), num);

    current_thread::run(
        future::poll_fn(move || {
            for _ in 0 .. num {
                // There are no other nodes, so the queries finish instantly.
                if let Ok(Async::Ready(Some(e))) = swarms[0].poll() {
                    if let KademliaEvent::BootstrapResult(r) = e {
                        assert!(r.is_ok(), "Unexpected error")
                    } else {
                        panic!("Unexpected event: {:?}", e)
                    }
                } else {
                    panic!("Expected event")
                }
            }
            Ok(Async::Ready(()))
        }))
}

