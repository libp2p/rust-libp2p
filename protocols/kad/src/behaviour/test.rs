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

use crate::{
    GetValueResult,
    Kademlia,
    KademliaOut,
    kbucket::{self, Distance},
    record::{Record, RecordStore},
};
use futures::{future, prelude::*};
use libp2p_core::{
    PeerId,
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
use rand::random;
use std::{collections::HashSet, iter::FromIterator, io, num::NonZeroU8, u64};
use tokio::runtime::Runtime;
use multihash::Hash;

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

        let kad = Kademlia::new(local_public_key.clone().into_peer_id());
        result.push(Swarm::new(transport, kad, local_public_key.into_peer_id()));
    }

    let mut i = 0;
    for s in result.iter_mut() {
        Swarm::listen_on(s, Protocol::Memory(port_base + i).into()).unwrap();
        i += 1
    }

    (port_base, result)
}

#[test]
fn query_iter() {
    fn distances(key: &kbucket::Key<PeerId>, peers: Vec<PeerId>) -> Vec<Distance> {
        peers.into_iter()
            .map(kbucket::Key::from)
            .map(|k| k.distance(key))
            .collect()
    }

    fn run(n: usize) {
        // Build `n` nodes. Node `n` knows about node `n-1`, node `n-1` knows about node `n-2`, etc.
        // Node `n` is queried for a random peer and should return nodes `1..n-1` sorted by
        // their distances to that peer.

        let (port_base, mut swarms) = build_nodes(n);
        let swarm_ids: Vec<_> = swarms.iter().map(Swarm::local_peer_id).cloned().collect();

        // Connect each swarm in the list to its predecessor in the list.
        for (i, (swarm, peer)) in &mut swarms.iter_mut().skip(1).zip(swarm_ids.clone()).enumerate() {
            swarm.add_address(&peer, Protocol::Memory(port_base + i as u64).into())
        }

        // Ask the last peer in the list to search a random peer. The search should
        // propagate backwards through the list of peers.
        let search_target = PeerId::random();
        let search_target_key = kbucket::Key::from(search_target.clone());
        swarms.last_mut().unwrap().find_node(search_target.clone());

        // Set up expectations.
        let expected_swarm_id = swarm_ids.last().unwrap().clone();
        let expected_peer_ids: Vec<_> = swarm_ids.iter().cloned().take(n - 1).collect();
        let mut expected_distances = distances(&search_target_key, expected_peer_ids.clone());
        expected_distances.sort();

        // Run test
        Runtime::new().unwrap().block_on(
            future::poll_fn(move || -> Result<_, io::Error> {
                for (i, swarm) in swarms.iter_mut().enumerate() {
                    loop {
                        match swarm.poll().unwrap() {
                            Async::Ready(Some(KademliaOut::FindNodeResult {
                                key, closer_peers
                            })) => {
                                assert_eq!(key, search_target);
                                assert_eq!(swarm_ids[i], expected_swarm_id);
                                assert!(expected_peer_ids.iter().all(|p| closer_peers.contains(p)));
                                let key = kbucket::Key::from(key);
                                assert_eq!(expected_distances, distances(&key, closer_peers));
                                return Ok(Async::Ready(()));
                            }
                            Async::Ready(_) => (),
                            Async::NotReady => break,
                        }
                    }
                }
                Ok(Async::NotReady)
            }))
            .unwrap()
    }

    for n in 2..=8 { run(n) }
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
    swarms[0].find_node(search_target.clone());

    Runtime::new().unwrap().block_on(
        future::poll_fn(move || -> Result<_, io::Error> {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::FindNodeResult { key, closer_peers })) => {
                            assert_eq!(key, search_target);
                            assert_eq!(closer_peers.len(), 0);
                            return Ok(Async::Ready(()));
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
        .unwrap();
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
    swarms[1].find_node(search_target.clone());

    Runtime::new().unwrap().block_on(
        future::poll_fn(move || -> Result<_, io::Error> {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::FindNodeResult { key, closer_peers })) => {
                            assert_eq!(key, search_target);
                            assert_eq!(closer_peers.len(), 1);
                            assert_eq!(closer_peers[0], first_peer_id);
                            return Ok(Async::Ready(()));
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
        .unwrap();
}


#[test]
fn get_value_not_found() {
    let (port_base, mut swarms) = build_nodes(3);

    let swarm_ids: Vec<_> = swarms.iter()
        .map(|swarm| Swarm::local_peer_id(&swarm).clone()).collect();

    swarms[0].add_address(&swarm_ids[1], Protocol::Memory(port_base + 1).into());
    swarms[1].add_address(&swarm_ids[2], Protocol::Memory(port_base + 2).into());

    let target_key = multihash::encode(Hash::SHA2256, &vec![1,2,3]).unwrap();
    let num_results = NonZeroU8::new(1).unwrap();
    swarms[0].get_value(&target_key, num_results);

    Runtime::new().unwrap().block_on(
        future::poll_fn(move || -> Result<_, io::Error> {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::GetValueResult(result))) => {
                            if let GetValueResult::NotFound { closest_peers} = result {
                                assert_eq!(closest_peers.len(), 2);
                                assert!(closest_peers.contains(&swarm_ids[1]));
                                assert!(closest_peers.contains(&swarm_ids[2]));
                                return Ok(Async::Ready(()));
                            } else {
                                panic!("Expected GetValueResult::NotFound event");
                            }
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
        .unwrap()
}

#[test]
fn put_value() {
    fn run() {
        // Build a test that checks if PUT_VALUE gets correctly propagated in
        // a nontrivial topology:
        //                [31]
        //               /    \
        //             [29]  [30]
        //            /|\      /|\
        //         [0]..[14] [15]..[28]
        //
        // Nodes [29] and [30] have less than kbuckets::MAX_NODES_PER_BUCKET
        // peers to avoid the situation when the bucket may be overflowed and
        // some of the connections are dropped from the routing table
        let (port_base, mut swarms) = build_nodes(32);

        let swarm_ids: Vec<_> = swarms.iter()
            .map(|swarm| Swarm::local_peer_id(&swarm).clone()).collect();

        // Connect swarm[30] to each swarm in swarms[..15]
        for (i, peer) in swarm_ids.iter().take(15).enumerate() {
            swarms[30].add_address(&peer, Protocol::Memory(port_base + i as u64).into());
        }

        // Connect swarm[29] to each swarm in swarms[15..29]
        for (i, peer) in swarm_ids.iter().skip(15).take(14).enumerate() {
            swarms[29].add_address(&peer, Protocol::Memory(port_base + (i + 15) as u64).into());
        }

        // Connect swarms[31] to swarms[29, 30]
        swarms[31].add_address(&swarm_ids[30], Protocol::Memory(port_base + 30 as u64).into());
        swarms[31].add_address(&swarm_ids[29], Protocol::Memory(port_base + 29 as u64).into());

        let target_key = multihash::encode(Hash::SHA2256, &vec![1,2,3]).unwrap();

        let mut sorted_peer_ids: Vec<_> = swarm_ids
            .iter()
            .map(|id| (id.clone(), kbucket::Key::from(id.clone()).distance(&kbucket::Key::from(target_key.clone()))))
            .collect();

        sorted_peer_ids.sort_by(|(_, d1), (_, d2)| d1.cmp(d2));

        let closest: HashSet<PeerId> = HashSet::from_iter(sorted_peer_ids.into_iter().map(|(id, _)| id));

        swarms[31].put_value(target_key.clone(), vec![4,5,6]);

        Runtime::new().unwrap().block_on(
            future::poll_fn(move || -> Result<_, io::Error> {
                let mut check_results = false;
                for swarm in &mut swarms {
                    loop {
                        match swarm.poll().unwrap() {
                            Async::Ready(Some(KademliaOut::PutValueResult{ .. })) => {
                                check_results = true;
                            }
                            Async::Ready(_) => (),
                            Async::NotReady => break,
                        }
                    }
                }

                if check_results {
                    let mut have: HashSet<_> = Default::default();

                    for (i, swarm) in swarms.iter().take(31).enumerate() {
                        if swarm.records.get(&target_key).is_some() {
                            have.insert(swarm_ids[i].clone());
                        }
                    }

                    let intersection: HashSet<_> = have.intersection(&closest).collect();

                    assert_eq!(have.len(), kbucket::MAX_NODES_PER_BUCKET);
                    assert_eq!(intersection.len(), kbucket::MAX_NODES_PER_BUCKET);
                    return Ok(Async::Ready(()));
                }

                Ok(Async::NotReady)
            }))
            .unwrap()
    }
    for _ in 0 .. 10 {
        run();
    }
}

#[test]
fn get_value() {
    let (port_base, mut swarms) = build_nodes(3);

    let swarm_ids: Vec<_> = swarms.iter()
        .map(|swarm| Swarm::local_peer_id(&swarm).clone()).collect();

    swarms[0].add_address(&swarm_ids[1], Protocol::Memory(port_base + 1).into());
    swarms[1].add_address(&swarm_ids[2], Protocol::Memory(port_base + 2).into());
    let target_key = multihash::encode(Hash::SHA2256, &vec![1,2,3]).unwrap();
    let target_value = vec![4,5,6];

    let num_results = NonZeroU8::new(1).unwrap();
    swarms[1].records.put(Record {
        key: target_key.clone(),
        value: target_value.clone()
    }).unwrap();
    swarms[0].get_value(&target_key, num_results);

    Runtime::new().unwrap().block_on(
        future::poll_fn(move || -> Result<_, io::Error> {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::GetValueResult(result))) => {
                            if let GetValueResult::Found { results } = result {
                                assert_eq!(results.len(), 1);
                                let record = results.first().unwrap();
                                assert_eq!(record.key, target_key);
                                assert_eq!(record.value, target_value);
                                return Ok(Async::Ready(()));
                            } else {
                                panic!("Expected GetValueResult::Found event");
                            }
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
        .unwrap()
}

#[test]
fn get_value_multiple() {
    // Check that if we have responses from multiple peers, a correct number of
    // results is returned.
    let num_results = NonZeroU8::new(10).unwrap();
    let (port_base, mut swarms) = build_nodes(2 + num_results.get() as usize);

    let swarm_ids: Vec<_> = swarms.iter()
        .map(|swarm| Swarm::local_peer_id(&swarm).clone()).collect();

    let target_key = multihash::encode(Hash::SHA2256, &vec![1,2,3]).unwrap();
    let target_value = vec![4,5,6];

    for (i, swarm_id) in swarm_ids.iter().skip(1).enumerate() {
        swarms[i + 1].records.put(Record {
            key: target_key.clone(),
            value: target_value.clone()
        }).unwrap();
        swarms[0].add_address(&swarm_id, Protocol::Memory(port_base + (i + 1) as u64).into());
    }

    swarms[0].records.put(Record { key: target_key.clone(), value: target_value.clone() }).unwrap();
    swarms[0].get_value(&target_key, num_results);

    Runtime::new().unwrap().block_on(
        future::poll_fn(move || -> Result<_, io::Error> {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::GetValueResult(result))) => {
                            if let GetValueResult::Found { results } = result {
                                assert_eq!(results.len(), num_results.get() as usize);
                                let record = results.first().unwrap();
                                assert_eq!(record.key, target_key);
                                assert_eq!(record.value, target_value);
                                return Ok(Async::Ready(()));
                            } else {
                                panic!("Expected GetValueResult::Found event");
                            }
                        }
                        Async::Ready(_) => (),
                        Async::NotReady => break,
                    }
                }
            }

            Ok(Async::NotReady)
        }))
        .unwrap()
}
