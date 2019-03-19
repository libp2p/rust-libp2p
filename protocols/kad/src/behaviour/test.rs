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

use crate::{Kademlia, KademliaOut};
use futures::prelude::*;
use libp2p_core::{upgrade, upgrade::InboundUpgradeExt, upgrade::OutboundUpgradeExt, PeerId, Swarm, Transport};
use libp2p_core::{nodes::Substream, transport::boxed::Boxed, muxing::StreamMuxerBox};
use std::io;

/// Builds swarms, each listening on a port. Does *not* connect the nodes together.
/// This is to be used only for testing, and a panic will happen if something goes wrong.
fn build_nodes(num: usize)
    -> Vec<Swarm<Boxed<(PeerId, StreamMuxerBox), io::Error>, Kademlia<Substream<StreamMuxerBox>>>>
{
    let mut result: Vec<Swarm<_, _>> = Vec::with_capacity(num);

    for _ in 0..num {
        // TODO: make creating the transport more elegant ; literaly half of the code of the test
        //       is about creating the transport
        let local_key = libp2p_core::identity::Keypair::generate_ed25519();
        let local_public_key = local_key.public();
        let transport = libp2p_tcp::TcpConfig::new()
            .with_upgrade(libp2p_secio::SecioConfig::new(local_key))
            .and_then(move |out, endpoint| {
                let peer_id = out.remote_key.into_peer_id();
                let peer_id2 = peer_id.clone();
                let upgrade = libp2p_yamux::Config::default()
                    .map_inbound(move |muxer| (peer_id, muxer))
                    .map_outbound(move |muxer| (peer_id2, muxer));
                upgrade::apply(out.stream, upgrade, endpoint)
                    .map(|(id, muxer)| (id, StreamMuxerBox::new(muxer)))
            })
            .map_err(|_| panic!())
            .boxed();

        let kad = Kademlia::without_init(local_public_key.clone().into_peer_id());
        result.push(Swarm::new(transport, kad, local_public_key.into_peer_id()));
    }

    for s in result.iter_mut() {
        Swarm::listen_on(s, "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
    }

    result
}

#[test]
fn basic_find_node() {
    // Build two nodes. Node #2 only knows about node #1. Node #2 is asked for a random peer ID.
    // Node #2 must return the identity of node #1.

    let mut swarms = build_nodes(2);
    let first_peer_id = Swarm::local_peer_id(&swarms[0]).clone();

    // Connect second to first.
    {
        let listen_addr = Swarm::listeners(&swarms[0]).next().unwrap().clone();
        swarms[1].add_not_connected_address(&first_peer_id, listen_addr);
    }

    let search_target = PeerId::random();
    swarms[1].find_node(search_target.clone());

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(futures::future::poll_fn(move || -> Result<_, io::Error> {
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
fn direct_query() {
    // Build three nodes. Node #2 knows about node #1. Node #3 knows about node #2. Node #3 is
    // asked about a random peer and should return nodes #1 and #2.

    let mut swarms = build_nodes(3);

    let first_peer_id = Swarm::local_peer_id(&swarms[0]).clone();
    let second_peer_id = Swarm::local_peer_id(&swarms[1]).clone();

    // Connect second to first.
    {
        let listen_addr = Swarm::listeners(&swarms[0]).next().unwrap().clone();
        swarms[1].add_not_connected_address(&first_peer_id, listen_addr);
    }

    // Connect third to second.
    {
        let listen_addr = Swarm::listeners(&swarms[1]).next().unwrap().clone();
        swarms[2].add_not_connected_address(&second_peer_id, listen_addr);
    }

    // Ask third to search a random value.
    let search_target = PeerId::random();
    swarms[2].find_node(search_target.clone());

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(futures::future::poll_fn(move || -> Result<_, io::Error> {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::FindNodeResult { key, closer_peers })) => {
                            assert_eq!(key, search_target);
                            assert_eq!(closer_peers.len(), 2);
                            assert!((closer_peers[0] == first_peer_id) != (closer_peers[1] == first_peer_id));
                            assert!((closer_peers[0] == second_peer_id) != (closer_peers[1] == second_peer_id));
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
fn indirect_query() {
    // Build four nodes. Node #2 knows about node #1. Node #3 knows about node #2. Node #4 knows
    // about node #3. Node #4 is asked about a random peer and should return nodes #1, #2 and #3.

    let mut swarms = build_nodes(4);

    let first_peer_id = Swarm::local_peer_id(&swarms[0]).clone();
    let second_peer_id = Swarm::local_peer_id(&swarms[1]).clone();
    let third_peer_id = Swarm::local_peer_id(&swarms[2]).clone();

    // Connect second to first.
    {
        let listen_addr = Swarm::listeners(&swarms[0]).next().unwrap().clone();
        swarms[1].add_not_connected_address(&first_peer_id, listen_addr);
    }

    // Connect third to second.
    {
        let listen_addr = Swarm::listeners(&swarms[1]).next().unwrap().clone();
        swarms[2].add_not_connected_address(&second_peer_id, listen_addr);
    }

    // Connect fourth to third.
    {
        let listen_addr = Swarm::listeners(&swarms[2]).next().unwrap().clone();
        swarms[3].add_not_connected_address(&third_peer_id, listen_addr);
    }

    // Ask fourth to search a random value.
    let search_target = PeerId::random();
    swarms[3].find_node(search_target.clone());

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(futures::future::poll_fn(move || -> Result<_, io::Error> {
            for swarm in &mut swarms {
                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::FindNodeResult { key, closer_peers })) => {
                            assert_eq!(key, search_target);
                            assert_eq!(closer_peers.len(), 3);
                            assert_eq!(closer_peers.iter().filter(|p| **p == first_peer_id).count(), 1);
                            assert_eq!(closer_peers.iter().filter(|p| **p == second_peer_id).count(), 1);
                            assert_eq!(closer_peers.iter().filter(|p| **p == third_peer_id).count(), 1);
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
fn unresponsive_not_returned_direct() {
    // Build one node. It contains fake addresses to non-existing nodes. We ask it to find a
    // random peer. We make sure that no fake address is returned.

    let mut swarms = build_nodes(1);

    // Add fake addresses.
    for _ in 0 .. 10 {
        swarms[0].add_not_connected_address(
            &PeerId::random(),
            libp2p_core::multiaddr::multiaddr![Udp(10u16)]
        );
    }

    // Ask first to search a random value.
    let search_target = PeerId::random();
    swarms[0].find_node(search_target.clone());

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(futures::future::poll_fn(move || -> Result<_, io::Error> {
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
    // non-existing nodes. We ask node #1 to find a random peer. We make sure that no fake address
    // is returned.

    let mut swarms = build_nodes(2);

    // Add fake addresses to first.
    let first_peer_id = Swarm::local_peer_id(&swarms[0]).clone();
    for _ in 0 .. 10 {
        swarms[0].add_not_connected_address(
            &PeerId::random(),
            libp2p_core::multiaddr::multiaddr![Udp(10u16)]
        );
    }

    // Connect second to first.
    {
        let listen_addr = Swarm::listeners(&swarms[0]).next().unwrap().clone();
        swarms[1].add_not_connected_address(&first_peer_id, listen_addr);
    }

    // Ask second to search a random value.
    let search_target = PeerId::random();
    swarms[1].find_node(search_target.clone());

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(futures::future::poll_fn(move || -> Result<_, io::Error> {
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
