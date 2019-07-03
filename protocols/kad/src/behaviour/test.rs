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

use crate::kbucket::Distance;
use futures::future;
use libp2p_core::{
    Swarm,
    Transport,
    identity,
    transport::{MemoryTransport, boxed::Boxed},
    nodes::Substream,
    multiaddr::{Protocol, multiaddr},
    muxing::StreamMuxerBox,
    upgrade,
};
use libp2p_secio::SecioConfig;
use libp2p_yamux as yamux;
use rand::{Rng, random, thread_rng};
use std::{collections::HashSet, iter::FromIterator, io, num::NonZeroU8, u64};
use tokio::runtime::current_thread;
use multihash::Hash::SHA2256;

type TestSwarm = Swarm<
    Boxed<(PeerId, StreamMuxerBox), io::Error>,
    Kademlia<Substream<StreamMuxerBox>>
>;

/// Builds swarms, each listening on a port. Does *not* connect the nodes together.
fn build_nodes(num: usize) -> (u64, Vec<TestSwarm>) {
    let port_base = 1 + random::<u64>() % (u64::MAX - num as u64);
    let mut result: Vec<Swarm<_, _>> = Vec::with_capacity(num);

    for _ in 0 .. num {
        // TODO: make creating the transport more elegant ; literaly half of the code of the test
        //       is about creating the transport
        let local_key = identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = MemoryTransport::default()
            .with_upgrade(SecioConfig::new(local_key))
            .and_then(move |out, endpoint| {
                let peer_id = out.remote_key.into_peer_id();
                let yamux = yamux::Config::default();
                upgrade::apply(out.stream, yamux, endpoint)
                    .map(|muxer| (peer_id, StreamMuxerBox::new(muxer)))
            })
            .map_err(|e| panic!("Failed to create transport: {:?}", e))
            .boxed();

        let cfg = KademliaConfig::new(local_public_key.clone().into_peer_id());
        let kad = Kademlia::new(cfg);
        result.push(Swarm::new(transport, kad, local_public_key.into_peer_id()));
    }

    let mut i = 0;
    for s in result.iter_mut() {
        Swarm::listen_on(s, Protocol::Memory(port_base + i).into()).unwrap();
        i += 1
    }

    (port_base, result)
}

fn build_connected_nodes(total: usize, step: usize) -> (Vec<PeerId>, Vec<TestSwarm>) {
    let (port_base, mut swarms) = build_nodes(total);
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
    fn run<G: rand::Rng>(rng: &mut G) {
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

    fn run<G: Rng>(rng: &mut G) {
        let num_total = rng.gen_range(2, 20);
        let (swarm_ids, mut swarms) = build_connected_nodes(num_total, 1);

        // Ask the first peer in the list to search a random peer. The search should
        // propagate forwards through the list of peers.
        let search_target = PeerId::random();
        let search_target_key = kbucket::Key::from(search_target.clone());
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
                                assert_eq!(ok.key, search_target);
                                assert_eq!(swarm_ids[i], expected_swarm_id);
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
                            assert_eq!(ok.key, search_target);
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
        swarms[0].add_address(
            &PeerId::random(),
            multiaddr![Udp(10u16)]
        );
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
                            assert_eq!(ok.key, search_target);
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

    let target_key = multihash::encode(SHA2256, &vec![1,2,3]).unwrap();
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
fn put_value() {
    fn run<G: rand::Rng>(rng: &mut G) {
        let num_total = rng.gen_range(21, 40);
        let num_group = rng.gen_range(1, usize::min(num_total, kbucket::K_VALUE));
        let (swarm_ids, mut swarms) = build_connected_nodes(num_total, num_group);

        let key = multihash::encode(SHA2256, &vec![1,2,3]).unwrap();
        let bucket_key = kbucket::Key::from(key.clone());

        let mut sorted_peer_ids: Vec<_> = swarm_ids
            .iter()
            .map(|id| (id.clone(), kbucket::Key::from(id.clone()).distance(&bucket_key)))
            .collect();

        sorted_peer_ids.sort_by(|(_, d1), (_, d2)| d1.cmp(d2));

        let closest = HashSet::from_iter(sorted_peer_ids.into_iter().map(|(id, _)| id));

        let record = Record { key: key.clone(), value: vec![4,5,6] };
        swarms[0].put_record(record, Quorum::All);

        current_thread::run(
            future::poll_fn(move || {
                let mut check_results = false;
                for swarm in &mut swarms {
                    loop {
                        match swarm.poll().unwrap() {
                            Async::Ready(Some(KademliaEvent::PutRecordResult(Ok(_)))) => {
                                check_results = true;
                            }
                            Async::Ready(_) => (),
                            Async::NotReady => break,
                        }
                    }
                }

                if check_results {
                    let mut have: HashSet<_> = Default::default();

                    for (i, swarm) in swarms.iter().skip(1).enumerate() {
                        if swarm.records.get(&key).is_some() {
                            have.insert(swarm_ids[i].clone());
                        }
                    }

                    let intersection: HashSet<_> = have.intersection(&closest).collect();

                    assert_eq!(have.len(), kbucket::K_VALUE);
                    assert_eq!(intersection.len(), kbucket::K_VALUE);

                    return Ok(Async::Ready(()));
                }

                Ok(Async::NotReady)
            }))
    }

    let mut rng = thread_rng();
    for _ in 0 .. 10 {
        run(&mut rng);
    }
}

#[test]
fn get_value() {
    let (port_base, mut swarms) = build_nodes(3);

    let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

    swarms[0].add_address(&swarm_ids[1], Protocol::Memory(port_base + 1).into());
    swarms[1].add_address(&swarm_ids[2], Protocol::Memory(port_base + 2).into());

    let record = Record {
        key: multihash::encode(SHA2256, &vec![1,2,3]).unwrap(),
        value: vec![4,5,6]
    };

    swarms[1].records.put(record.clone()).unwrap();
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
fn get_value_multiple() {
    // Check that if we have responses from multiple peers, a correct number of
    // results is returned.
    let num_nodes = 12;
    let (_swarm_ids, mut swarms) = build_connected_nodes(num_nodes, num_nodes);
    let num_results = 10;

    let record = Record {
        key: multihash::encode(SHA2256, &vec![1,2,3]).unwrap(),
        value: vec![4,5,6],
    };

    for i in 0 .. num_nodes {
        swarms[i].records.put(record.clone()).unwrap();
    }

    let quorum = Quorum::N(NonZeroU8::new(num_results as u8).unwrap());
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
