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

/// Builds swarms. The second one and further have the first one as bootstrap node.
/// This is to be used only for testing, and a panic will happen if something goes wrong.
fn build_nodes(num: usize)
    -> Vec<Swarm<Boxed<(PeerId, StreamMuxerBox), io::Error>, Kademlia<Substream<StreamMuxerBox>>>>
{
    let mut result: Vec<Swarm<_, _>> = Vec::with_capacity(num);

    for _ in 0..num {
        // TODO: make creating the transport more elegant ; literaly half of the code of the test
        //       is about creating the transport
        let local_key = libp2p_secio::SecioKeyPair::ed25519_generated().unwrap();
        let local_public_key = local_key.to_public_key();
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

    let listen = Swarm::listen_on(&mut result[0], "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
    let first_peer_id = Swarm::local_peer_id(&result[0]).clone();
    for s in result.iter_mut().skip(1) {
        Swarm::listen_on(s, "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
        s.add_not_connected_address(&first_peer_id, listen.clone());
    }

    result
}

#[test]
fn basic_find_node() {
    let mut swarms = build_nodes(2);

    let first_peer_id = Swarm::local_peer_id(&swarms[0]).clone();
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
fn find_node_yields_other() {
    let mut swarms = build_nodes(3);

    let first_peer_id = Swarm::local_peer_id(&swarms[0]).clone();
    let second_peer_id = Swarm::local_peer_id(&swarms[1]).clone();

    let search_target1 = PeerId::random();
    let search_target2 = PeerId::random();
    swarms[1].find_node(search_target1.clone());

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(futures::future::poll_fn(move || -> Result<_, io::Error> {
            let mut start_second_query = false;
            for (swarm_n, swarm) in swarms.iter_mut().enumerate() {
                if start_second_query && swarm_n == 2 {
                    swarm.find_node(search_target2.clone());
                    start_second_query = false;
                }

                loop {
                    match swarm.poll().unwrap() {
                        Async::Ready(Some(KademliaOut::FindNodeResult { key, closer_peers })) => {
                            if swarm_n == 1 {
                                assert_eq!(key, search_target1.clone());
                                start_second_query = true;
                            } else if swarm_n == 2 {
                                assert_eq!(key, search_target2.clone());
                                assert_eq!(closer_peers.len(), 2);
                                assert!(closer_peers[0] == first_peer_id || closer_peers[1] == first_peer_id);
                                assert!(closer_peers[0] == second_peer_id || closer_peers[1] == second_peer_id);
                                return Ok(Async::Ready(()));
                            } else {
                                panic!()
                            }
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
