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

use crate::{ALPHA_VALUE, K_VALUE};
use crate::kbucket::Distance;
use crate::record::store::MemoryStore;
use futures::{
    prelude::*,
    executor::block_on,
    future::poll_fn,
};
use libp2p_core::{
    PeerId,
    Transport,
    identity,
    transport::MemoryTransport,
    multiaddr::{Protocol, Multiaddr, multiaddr},
    muxing::StreamMuxerBox,
    upgrade
};
use libp2p_secio::SecioConfig;
use libp2p_swarm::Swarm;
use libp2p_yamux as yamux;
use quickcheck::*;
use rand::{Rng, random, thread_rng};
use std::{collections::{HashSet, HashMap}, io, num::NonZeroUsize, u64};
use multihash::{wrap, Code, Multihash};

type TestSwarm = Swarm<Kademlia<MemoryStore>>;

fn build_node() -> (Multiaddr, TestSwarm) {
    build_node_with_config(Default::default())
}

fn build_node_with_config(cfg: KademliaConfig) -> (Multiaddr, TestSwarm) {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let transport = MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(SecioConfig::new(local_key))
        .multiplex(yamux::Config::default())
        .map(|(p, m), _| (p, StreamMuxerBox::new(m)))
        .map_err(|e| -> io::Error { panic!("Failed to create transport: {:?}", e); })
        .boxed();

    let local_id = local_public_key.clone().into_peer_id();
    let store = MemoryStore::new(local_id.clone());
    let behaviour = Kademlia::with_config(local_id.clone(), store, cfg.clone());

    let mut swarm = Swarm::new(transport, behaviour, local_id);

    let address: Multiaddr = Protocol::Memory(random::<u64>()).into();
    Swarm::listen_on(&mut swarm, address.clone()).unwrap();

    (address, swarm)
}

/// Builds swarms, each listening on a port. Does *not* connect the nodes together.
fn build_nodes(num: usize) -> Vec<(Multiaddr, TestSwarm)> {
    build_nodes_with_config(num, Default::default())
}

/// Builds swarms, each listening on a port. Does *not* connect the nodes together.
fn build_nodes_with_config(num: usize, cfg: KademliaConfig) -> Vec<(Multiaddr, TestSwarm)> {
    (0..num).map(|_| build_node_with_config(cfg.clone())).collect()
}

fn build_connected_nodes(total: usize, step: usize) -> Vec<(Multiaddr, TestSwarm)> {
    build_connected_nodes_with_config(total, step, Default::default())
}

fn build_connected_nodes_with_config(total: usize, step: usize, cfg: KademliaConfig)
    -> Vec<(Multiaddr, TestSwarm)>
{
    let mut swarms = build_nodes_with_config(total, cfg);
    let swarm_ids: Vec<_> = swarms.iter()
        .map(|(addr, swarm)| (addr.clone(), Swarm::local_peer_id(swarm).clone()))
        .collect();

    let mut i = 0;
    for (j, (addr, peer_id)) in swarm_ids.iter().enumerate().skip(1) {
        if i < swarm_ids.len() {
            swarms[i].1.add_address(peer_id, addr.clone());
        }
        if j % step == 0 {
            i += step;
        }
    }

    swarms
}

fn build_fully_connected_nodes_with_config(total: usize, cfg: KademliaConfig)
    -> Vec<(Multiaddr, TestSwarm)>
{
    let mut swarms = build_nodes_with_config(total, cfg);
    let swarm_addr_and_peer_id: Vec<_> = swarms.iter()
        .map(|(addr, swarm)| (addr.clone(), Swarm::local_peer_id(swarm).clone()))
        .collect();

    for (_addr, swarm) in swarms.iter_mut() {
        for (addr, peer) in &swarm_addr_and_peer_id {
            swarm.add_address(&peer, addr.clone());
        }
    }

    swarms
}

fn random_multihash() -> Multihash {
    wrap(Code::Sha2_256, &thread_rng().gen::<[u8; 32]>())
}

#[test]
fn bootstrap() {
    fn run(rng: &mut impl Rng) {
        let num_total = rng.gen_range(2, 20);
        // When looking for the closest node to a key, Kademlia considers ALPHA_VALUE nodes to query
        // at initialization. If `num_groups` is larger than ALPHA_VALUE the remaining locally known
        // nodes will not be considered. Given that no other node is aware of them, they would be
        // lost entirely. To prevent the above restrict `num_groups` to be equal or smaller than
        // ALPHA_VALUE.
        let num_group = rng.gen_range(1, (num_total % ALPHA_VALUE.get()) + 2);

        let mut swarms = build_connected_nodes(num_total, num_group).into_iter()
            .map(|(_a, s)| s)
            .collect::<Vec<_>>();
        let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

        swarms[0].bootstrap();

        // Expected known peers
        let expected_known = swarm_ids.iter().skip(1).cloned().collect::<HashSet<_>>();

        // Run test
        block_on(
            poll_fn(move |ctx| {
                for (i, swarm) in swarms.iter_mut().enumerate() {
                    loop {
                        match swarm.poll_next_unpin(ctx) {
                            Poll::Ready(Some(KademliaEvent::BootstrapResult(Ok(ok)))) => {
                                assert_eq!(i, 0);
                                assert_eq!(ok.peer, swarm_ids[0]);
                                let known = swarm.kbuckets.iter()
                                    .map(|e| e.node.key.preimage().clone())
                                    .collect::<HashSet<_>>();
                                assert_eq!(expected_known, known);
                                return Poll::Ready(())
                            }
                            // Ignore any other event.
                            Poll::Ready(Some(_)) => (),
                            e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                            Poll::Pending => break,
                        }
                    }
                }
                Poll::Pending
            })
        )
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
        let mut swarms = build_connected_nodes(num_total, 1).into_iter()
            .map(|(_a, s)| s)
            .collect::<Vec<_>>();
        let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

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
        block_on(
            poll_fn(move |ctx| {
                for (i, swarm) in swarms.iter_mut().enumerate() {
                    loop {
                        match swarm.poll_next_unpin(ctx) {
                            Poll::Ready(Some(KademliaEvent::GetClosestPeersResult(Ok(ok)))) => {
                                assert_eq!(&ok.key[..], search_target.as_bytes());
                                assert_eq!(swarm_ids[i], expected_swarm_id);
                                assert_eq!(swarm.queries.size(), 0);
                                assert!(expected_peer_ids.iter().all(|p| ok.peers.contains(p)));
                                let key = kbucket::Key::new(ok.key);
                                assert_eq!(expected_distances, distances(&key, ok.peers));
                                return Poll::Ready(());
                            }
                            // Ignore any other event.
                            Poll::Ready(Some(_)) => (),
                            e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                            Poll::Pending => break,
                        }
                    }
                }
                Poll::Pending
            })
        )
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

    let mut swarms = build_nodes(1).into_iter()
        .map(|(_a, s)| s)
        .collect::<Vec<_>>();

    // Add fake addresses.
    for _ in 0 .. 10 {
        swarms[0].add_address(&PeerId::random(), Protocol::Udp(10u16).into());
    }

    // Ask first to search a random value.
    let search_target = PeerId::random();
    swarms[0].get_closest_peers(search_target.clone());

    block_on(
        poll_fn(move |ctx| {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll_next_unpin(ctx) {
                        Poll::Ready(Some(KademliaEvent::GetClosestPeersResult(Ok(ok)))) => {
                            assert_eq!(&ok.key[..], search_target.as_bytes());
                            assert_eq!(ok.peers.len(), 0);
                            return Poll::Ready(());
                        }
                        // Ignore any other event.
                        Poll::Ready(Some(_)) => (),
                        e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                        Poll::Pending => break,
                    }
                }
            }

            Poll::Pending
        })
    )
}

#[test]
fn unresponsive_not_returned_indirect() {
    // Build two nodes. Node #2 knows about node #1. Node #1 contains fake addresses to
    // non-existing nodes. We ask node #2 to find a random peer. We make sure that no fake address
    // is returned.

    let mut swarms = build_nodes(2);

    // Add fake addresses to first.
    for _ in 0 .. 10 {
        swarms[0].1.add_address(&PeerId::random(), multiaddr![Udp(10u16)]);
    }

    // Connect second to first.
    let first_peer_id = Swarm::local_peer_id(&swarms[0].1).clone();
    let first_address = swarms[0].0.clone();
    swarms[1].1.add_address(&first_peer_id, first_address);

    // Drop the swarm addresses.
    let mut swarms = swarms.into_iter().map(|(_addr, swarm)| swarm).collect::<Vec<_>>();

    // Ask second to search a random value.
    let search_target = PeerId::random();
    swarms[1].get_closest_peers(search_target.clone());

    block_on(
        poll_fn(move |ctx| {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll_next_unpin(ctx) {
                        Poll::Ready(Some(KademliaEvent::GetClosestPeersResult(Ok(ok)))) => {
                            assert_eq!(&ok.key[..], search_target.as_bytes());
                            assert_eq!(ok.peers.len(), 1);
                            assert_eq!(ok.peers[0], first_peer_id);
                            return Poll::Ready(());
                        }
                        // Ignore any other event.
                        Poll::Ready(Some(_)) => (),
                        e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                        Poll::Pending => break,
                    }
                }
            }

            Poll::Pending
        })
    )
}

#[test]
fn get_record_not_found() {
    let mut swarms = build_nodes(3);

    let swarm_ids: Vec<_> = swarms.iter()
        .map(|(_addr, swarm)| Swarm::local_peer_id(swarm))
        .cloned()
        .collect();

    let (second, third) = (swarms[1].0.clone(), swarms[2].0.clone());
    swarms[0].1.add_address(&swarm_ids[1], second);
    swarms[1].1.add_address(&swarm_ids[2], third);

    // Drop the swarm addresses.
    let mut swarms = swarms.into_iter().map(|(_addr, swarm)| swarm).collect::<Vec<_>>();

    let target_key = record::Key::from(random_multihash());
    swarms[0].get_record(&target_key, Quorum::One);

    block_on(
        poll_fn(move |ctx| {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll_next_unpin(ctx) {
                        Poll::Ready(Some(KademliaEvent::GetRecordResult(Err(e)))) => {
                            if let GetRecordError::NotFound { key, closest_peers, } = e {
                                assert_eq!(key, target_key);
                                assert_eq!(closest_peers.len(), 2);
                                assert!(closest_peers.contains(&swarm_ids[1]));
                                assert!(closest_peers.contains(&swarm_ids[2]));
                                return Poll::Ready(());
                            } else {
                                panic!("Unexpected error result: {:?}", e);
                            }
                        }
                        // Ignore any other event.
                        Poll::Ready(Some(_)) => (),
                        e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                        Poll::Pending => break,
                    }
                }
            }

            Poll::Pending
        })
    )
}

/// A node joining a fully connected network via a single bootnode should be able to put a record to
/// the X closest nodes of the network where X is equal to the configured replication factor.
#[test]
fn put_record() {
    fn prop(replication_factor: usize, records: Vec<Record>) {
        let replication_factor = NonZeroUsize::new(replication_factor % (K_VALUE.get() / 2) + 1).unwrap();
        let num_total = replication_factor.get() * 2;

        let mut config = KademliaConfig::default();
        config.set_replication_factor(replication_factor);

        let mut swarms = {
            let mut fully_connected_swarms = build_fully_connected_nodes_with_config(
                num_total - 1,
                config.clone(),
            );

            let mut single_swarm = build_node_with_config(config);
            single_swarm.1.add_address(
                Swarm::local_peer_id(&fully_connected_swarms[0].1),
                fully_connected_swarms[0].0.clone(),
            );

            let mut swarms = vec![single_swarm];
            swarms.append(&mut fully_connected_swarms);

            // Drop the swarm addresses.
            swarms.into_iter().map(|(_addr, swarm)| swarm).collect::<Vec<_>>()
        };

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

        block_on(
            poll_fn(move |ctx| loop {
                // Poll all swarms until they are "Pending".
                for swarm in &mut swarms {
                    loop {
                        match swarm.poll_next_unpin(ctx) {
                            Poll::Ready(Some(KademliaEvent::PutRecordResult(res))) |
                            Poll::Ready(Some(KademliaEvent::RepublishRecordResult(res))) => {
                                match res {
                                    Err(e) => panic!("{:?}", e),
                                    Ok(ok) => {
                                        assert!(records.contains_key(&ok.key));
                                        let record = swarm.store.get(&ok.key).unwrap();
                                        results.push(record.into_owned());
                                    }
                                }
                            }
                            // Ignore any other event.
                            Poll::Ready(Some(_)) => (),
                            e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                            Poll::Pending => break,
                        }
                    }
                }

                // All swarms are Pending and not enough results have been collected
                // so far, thus wait to be polled again for further progress.
                if results.len() != records.len() {
                    return Poll::Pending
                }

                // Consume the results, checking that each record was replicated
                // correctly to the closest peers to the key.
                while let Some(r) = results.pop() {
                    let expected = records.get(&r.key).unwrap();

                    assert_eq!(r.key, expected.key);
                    assert_eq!(r.value, expected.value);
                    assert_eq!(r.expires, expected.expires);
                    assert_eq!(r.publisher.as_ref(), Some(Swarm::local_peer_id(&swarms[0])));

                    let key = kbucket::Key::new(r.key.clone());
                    let mut expected = swarms.iter()
                        .skip(1)
                        .map(Swarm::local_peer_id)
                        .cloned()
                        .collect::<Vec<_>>();
                    expected.sort_by(|id1, id2|
                        kbucket::Key::new(id1.clone()).distance(&key).cmp(
                            &kbucket::Key::new(id2.clone()).distance(&key)));

                    let expected = expected
                        .into_iter()
                        .take(replication_factor.get())
                        .collect::<HashSet<_>>();

                    let actual = swarms.iter()
                        .skip(1)
                        .filter_map(|swarm|
                            if swarm.store.get(key.preimage()).is_some() {
                                Some(Swarm::local_peer_id(swarm).clone())
                            } else {
                                None
                            })
                        .collect::<HashSet<_>>();

                    assert_eq!(actual.len(), replication_factor.get());

                    let actual_not_expected = actual.difference(&expected)
                        .collect::<Vec<&PeerId>>();
                    assert!(
                        actual_not_expected.is_empty(),
                        "Did not expect records to be stored on nodes {:?}.",
                        actual_not_expected,
                    );

                    let expected_not_actual = expected.difference(&actual)
                        .collect::<Vec<&PeerId>>();
                    assert!(expected_not_actual.is_empty(),
                           "Expected record to be stored on nodes {:?}.",
                           expected_not_actual,
                    );
                }

                if republished {
                    assert_eq!(swarms[0].store.records().count(), records.len());
                    assert_eq!(swarms[0].queries.size(), 0);
                    for k in records.keys() {
                        swarms[0].store.remove(&k);
                    }
                    assert_eq!(swarms[0].store.records().count(), 0);
                    // All records have been republished, thus the test is complete.
                    return Poll::Ready(());
                }

                // Tell the replication job to republish asap.
                swarms[0].put_record_job.as_mut().unwrap().asap(true);
                republished = true;
            })
        )
    }

    QuickCheck::new().tests(3).quickcheck(prop as fn(_,_) -> _)
}

#[test]
fn get_value() {
    let mut swarms = build_nodes(3);

    // Let first peer know of second peer and second peer know of third peer.
    for i in 0..2 {
        let (peer_id, address) = (Swarm::local_peer_id(&swarms[i+1].1).clone(), swarms[i+1].0.clone());
        swarms[i].1.add_address(&peer_id, address);
    }

    // Drop the swarm addresses.
    let mut swarms = swarms.into_iter().map(|(_addr, swarm)| swarm).collect::<Vec<_>>();

    let record = Record::new(random_multihash(), vec![4,5,6]);

    swarms[1].store.put(record.clone()).unwrap();
    swarms[0].get_record(&record.key, Quorum::One);

    block_on(
        poll_fn(move |ctx| {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll_next_unpin(ctx) {
                        Poll::Ready(Some(KademliaEvent::GetRecordResult(Ok(ok)))) => {
                            assert_eq!(ok.records.len(), 1);
                            assert_eq!(ok.records.first(), Some(&record));
                            return Poll::Ready(());
                        }
                        // Ignore any other event.
                        Poll::Ready(Some(_)) => (),
                        e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                        Poll::Pending => break,
                    }
                }
            }

            Poll::Pending
        })
    )
}

#[test]
fn get_value_many() {
    // TODO: Randomise
    let num_nodes = 12;
    let mut swarms = build_connected_nodes(num_nodes, 3).into_iter()
        .map(|(_addr, swarm)| swarm)
        .collect::<Vec<_>>();
    let num_results = 10;

    let record = Record::new(random_multihash(), vec![4,5,6]);

    for i in 0 .. num_nodes {
        swarms[i].store.put(record.clone()).unwrap();
    }

    let quorum = Quorum::N(NonZeroUsize::new(num_results).unwrap());
    swarms[0].get_record(&record.key, quorum);

    block_on(
        poll_fn(move |ctx| {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll_next_unpin(ctx) {
                        Poll::Ready(Some(KademliaEvent::GetRecordResult(Ok(ok)))) => {
                            assert_eq!(ok.records.len(), num_results);
                            assert_eq!(ok.records.first(), Some(&record));
                            return Poll::Ready(());
                        }
                        // Ignore any other event.
                        Poll::Ready(Some(_)) => (),
                        e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                        Poll::Pending => break,
                    }
                }
            }
            Poll::Pending
        })
    )
}

/// A node joining a fully connected network via a single bootnode should be able to add itself as a
/// provider to the X closest nodes of the network where X is equal to the configured replication
/// factor.
#[test]
fn add_provider() {
    fn prop(replication_factor: usize, keys: Vec<record::Key>) {
        let replication_factor = NonZeroUsize::new(replication_factor % (K_VALUE.get() / 2) + 1).unwrap();
        let num_total = replication_factor.get() * 2;

        let mut config = KademliaConfig::default();
        config.set_replication_factor(replication_factor);

        let mut swarms = {
            let mut fully_connected_swarms = build_fully_connected_nodes_with_config(
                num_total - 1,
                config.clone(),
            );

            let mut single_swarm = build_node_with_config(config);
            single_swarm.1.add_address(
                Swarm::local_peer_id(&fully_connected_swarms[0].1),
                fully_connected_swarms[0].0.clone(),
            );

            let mut swarms = vec![single_swarm];
            swarms.append(&mut fully_connected_swarms);

            // Drop addresses before returning.
            swarms.into_iter().map(|(_addr, swarm)| swarm).collect::<Vec<_>>()
        };

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

        block_on(
            poll_fn(move |ctx| loop {
                // Poll all swarms until they are "Pending".
                for swarm in &mut swarms {
                    loop {
                        match swarm.poll_next_unpin(ctx) {
                            Poll::Ready(Some(KademliaEvent::StartProvidingResult(res))) |
                            Poll::Ready(Some(KademliaEvent::RepublishProviderResult(res))) => {
                                match res {
                                    Err(e) => panic!(e),
                                    Ok(ok) => {
                                        assert!(keys.contains(&ok.key));
                                        results.push(ok.key);
                                    }
                                }
                            }
                            // Ignore any other event.
                            Poll::Ready(Some(_)) => (),
                            e @ Poll::Ready(_) => panic!("Unexpected return value: {:?}", e),
                            Poll::Pending => break,
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
                    return Poll::Pending
                }

                // A round of publishing is complete. Consume the results, checking that
                // each key was published to the `replication_factor` closest peers.
                while let Some(key) = results.pop() {
                    // Collect the nodes that have a provider record for `key`.
                    let actual = swarms.iter().skip(1)
                        .filter_map(|swarm|
                            if swarm.store.providers(&key).len() == 1 {
                                Some(Swarm::local_peer_id(&swarm).clone())
                            } else {
                                None
                            })
                        .collect::<HashSet<_>>();

                    if actual.len() != replication_factor.get() {
                        // Still waiting for some nodes to process the request.
                        results.push(key);
                        return Poll::Pending
                    }

                    let mut expected = swarms.iter()
                        .skip(1)
                        .map(Swarm::local_peer_id)
                        .cloned()
                        .collect::<Vec<_>>();
                    let kbucket_key = kbucket::Key::new(key);
                    expected.sort_by(|id1, id2|
                        kbucket::Key::new(id1.clone()).distance(&kbucket_key).cmp(
                            &kbucket::Key::new(id2.clone()).distance(&kbucket_key)));

                    let expected = expected
                        .into_iter()
                        .take(replication_factor.get())
                        .collect::<HashSet<_>>();

                    assert_eq!(actual, expected);
                }

                // One round of publishing is complete.
                assert!(results.is_empty());
                for swarm in &swarms {
                    assert_eq!(swarm.queries.size(), 0);
                }

                if republished {
                    assert_eq!(swarms[0].store.provided().count(), keys.len());
                    for k in &keys {
                        swarms[0].stop_providing(&k);
                    }
                    assert_eq!(swarms[0].store.provided().count(), 0);
                    // All records have been republished, thus the test is complete.
                    return Poll::Ready(());
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
    let (_addr, mut swarm) = build_node();
    let num = JOBS_MAX_QUERIES + 1;
    for _ in 0 .. num {
        swarm.bootstrap();
    }

    assert_eq!(swarm.queries.size(), num);

    block_on(
        poll_fn(move |ctx| {
            for _ in 0 .. num {
                // There are no other nodes, so the queries finish instantly.
                if let Poll::Ready(Some(e)) = swarm.poll_next_unpin(ctx) {
                    if let KademliaEvent::BootstrapResult(r) = e {
                        assert!(r.is_ok(), "Unexpected error")
                    } else {
                        panic!("Unexpected event: {:?}", e)
                    }
                } else {
                    panic!("Expected event")
                }
            }
            Poll::Ready(())
        })
    )
}

#[test]
fn exp_decr_expiration_overflow() {
    fn prop_no_panic(ttl: Duration, factor: u32) {
        exp_decrease(ttl, factor);
    }

    // Right shifting a u64 by >63 results in a panic.
    prop_no_panic(KademliaConfig::default().record_ttl.unwrap(), 64);

    quickcheck(prop_no_panic as fn(_, _))
}
