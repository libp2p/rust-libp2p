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

use futures::prelude::*;
use log::debug;
use quickcheck::{QuickCheck, TestResult};
use rand::{random, seq::SliceRandom, SeedableRng};
use std::{
    io::Error,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use libp2p_core::{
    identity,
    multiaddr::Protocol,
    muxing::StreamMuxerBox,
    transport::MemoryTransport,
    upgrade, Multiaddr, Transport,
};
use libp2p_gossipsub::{Gossipsub, GossipsubConfig, GossipsubEvent, Topic};
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::Swarm;
use libp2p_yamux as yamux;

struct Graph {
    pub nodes: Vec<(Multiaddr, Swarm<Gossipsub>)>,
}

impl Future for Graph {
    type Output = (Multiaddr, GossipsubEvent);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        for (addr, node) in &mut self.nodes {
            match node.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => return Poll::Ready((addr.clone(), event)),
                Poll::Ready(None) => panic!("unexpected None when polling nodes"),
                Poll::Pending => {}
            }
        }

        Poll::Pending
    }
}

impl Graph {
    fn new_connected(num_nodes: usize, seed: u64) -> Graph {
        if num_nodes == 0 {
            panic!("expecting at least one node");
        }

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let mut not_connected_nodes = std::iter::once(())
            .cycle()
            .take(num_nodes)
            .map(|_| build_node())
            .collect::<Vec<(Multiaddr, Swarm<Gossipsub>)>>();

        let mut connected_nodes = vec![not_connected_nodes.pop().unwrap()];

        while !not_connected_nodes.is_empty() {
            connected_nodes.shuffle(&mut rng);
            not_connected_nodes.shuffle(&mut rng);

            let mut next = not_connected_nodes.pop().unwrap();
            let connected_addr = &connected_nodes[0].0;

            // Memory transport can not handle addresses with `/p2p` suffix.
            let mut connected_addr_no_p2p = connected_addr.clone();
            let p2p_suffix_connected = connected_addr_no_p2p.pop();

            debug!(
                "Connect: {} -> {}",
                next.0.clone().pop().unwrap(),
                p2p_suffix_connected.unwrap()
            );

            Swarm::dial_addr(&mut next.1, connected_addr_no_p2p).unwrap();

            connected_nodes.push(next);
        }

        Graph {
            nodes: connected_nodes,
        }
    }

    /// Polls the graph and passes each event into the provided FnMut until it returns `true`.
    fn wait_for<F>(self, mut f: F) -> Self
    where
        F: FnMut(GossipsubEvent) -> bool,
    {
        // The future below should return self. Given that it is a FnMut and not a FnOnce, one needs
        // to wrap `self` in an Option, leaving a `None` behind after the final `Poll::Ready`.
        let mut this = Some(self);

        let fut = futures::future::poll_fn(move |cx| match &mut this {
            Some(graph) => loop {
                match graph.poll_unpin(cx) {
                    Poll::Ready((_addr, ev)) => {
                        if f(ev) {
                            return Poll::Ready(this.take().unwrap());
                        }
                    }
                    Poll::Pending => return Poll::Pending,
                }
            },
            None => panic!("future called after final return"),
        });

        let fut = async_std::future::timeout(Duration::from_secs(10), fut);

        futures::executor::block_on(fut).unwrap()
    }
}

fn build_node() -> (Multiaddr, Swarm<Gossipsub>) {
    let key = identity::Keypair::generate_ed25519();
    let public_key = key.public();

    let transport = MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(PlainText2Config {
            local_public_key: public_key.clone(),
        })
        .multiplex(yamux::Config::default())
        .map(|(p, m), _| (p, StreamMuxerBox::new(m)))
        .map_err(|e| -> Error { panic!("Failed to create transport: {:?}", e) })
        .boxed();

    let peer_id = public_key.clone().into_peer_id();
    let behaviour = Gossipsub::new(peer_id.clone(), GossipsubConfig::default());
    let mut swarm = Swarm::new(transport, behaviour, peer_id);

    let port = 1 + random::<u64>();
    let mut addr: Multiaddr = Protocol::Memory(port).into();
    Swarm::listen_on(&mut swarm, addr.clone()).unwrap();

    addr = addr.with(libp2p_core::multiaddr::Protocol::P2p(
        public_key.into_peer_id().into(),
    ));

    (addr, swarm)
}

#[test]
fn multi_hop_propagation() {
    let _ = env_logger::try_init();

    fn prop(num_nodes: usize, seed: u64) -> TestResult {
        if num_nodes < 2 || num_nodes > 100 {
            return TestResult::discard();
        }

        debug!("number nodes: {:?}, seed: {:?}", num_nodes, seed);

        let mut graph = Graph::new_connected(num_nodes, seed);
        let number_nodes = graph.nodes.len();

        // Subscribe each node to the same topic.
        let topic = Topic::new("test-net".into());
        for (_addr, node) in &mut graph.nodes {
            node.subscribe(topic.clone());
        }

        // Wait for all nodes to be subscribed.
        let mut subscribed = 0;
        graph = graph.wait_for(move |ev| {
            if let GossipsubEvent::Subscribed { .. } = ev {
                subscribed += 1;
                if subscribed == (number_nodes - 1) * 2 {
                    return true;
                }
            }

            false
        });

        // Publish a single message.
        graph.nodes[0].1.publish(&topic, vec![1, 2, 3]);

        // Wait for all nodes to receive the published message.
        let mut received_msgs = 0;
        graph.wait_for(move |ev| {
            if let GossipsubEvent::Message(..) = ev {
                received_msgs += 1;
                if received_msgs == number_nodes - 1 {
                    return true;
                }
            }

            false
        });

        TestResult::passed()
    }

    QuickCheck::new()
        .max_tests(10)
        .quickcheck(prop as fn(usize, u64) -> TestResult)
}
