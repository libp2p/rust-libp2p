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

use futures::stream::SelectAll;
use log::debug;
use quickcheck::{QuickCheck, TestResult};
use rand::{random, seq::SliceRandom, SeedableRng};
use std::{task::Poll, time::Duration};

use futures::{ready, StreamExt};
use libp2p::core::{
    identity, multiaddr::Protocol, transport::MemoryTransport, upgrade, Multiaddr, Transport,
};
use libp2p::gossipsub::{
    Gossipsub, GossipsubConfigBuilder, GossipsubEvent, IdentTopic as Topic, MessageAuthenticity,
    ValidationMode,
};
use libp2p::plaintext::PlainText2Config;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::yamux;

struct Graph {
    pub nodes: SelectAll<Swarm<Gossipsub>>,
}

impl Graph {
    fn new_connected(num_nodes: usize, seed: u64) -> Graph {
        if num_nodes == 0 {
            panic!("expecting at least one node");
        }

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let mut not_connected_nodes = (0..num_nodes)
            .map(|_| build_node())
            .collect::<Vec<(Multiaddr, Swarm<Gossipsub>)>>();

        let mut connected_nodes = vec![not_connected_nodes.pop().unwrap()];

        while !not_connected_nodes.is_empty() {
            connected_nodes.shuffle(&mut rng);
            not_connected_nodes.shuffle(&mut rng);

            let mut next = not_connected_nodes.pop().unwrap();
            let connected_addr = &connected_nodes[0].0;

            debug!(
                "Connect: {} -> {}",
                next.0.clone().pop().unwrap(),
                next.1.local_peer_id()
            );

            next.1.dial(connected_addr.clone()).unwrap();

            connected_nodes.push(next);
        }

        Graph {
            nodes: connected_nodes
                .into_iter()
                .map(|(_, swarm)| swarm)
                .collect(),
        }
    }

    /// Polls the graph and passes each event into the provided FnMut until the closure returns
    /// `true`.
    ///
    /// Returns [`true`] on success and [`false`] on timeout.
    fn wait_for<F: FnMut(&GossipsubEvent) -> bool>(&mut self, mut f: F) -> bool {
        let fut = futures::future::poll_fn(move |cx| loop {
            match ready!(self.nodes.poll_next_unpin(cx)) {
                Some(SwarmEvent::Behaviour(ev)) if f(&ev) => return Poll::Ready(()),
                Some(_) => continue,
                None => unreachable!(),
            }
        });

        let fut = async_std::future::timeout(Duration::from_secs(10), fut);

        futures::executor::block_on(fut).is_ok()
    }

    /// Polls the graph until Poll::Pending is obtained, completing the underlying polls.
    fn drain_poll(self) -> Self {
        // The future below should return self. Given that it is a FnMut and not a FnOnce, one needs
        // to wrap `self` in an Option, leaving a `None` behind after the final `Poll::Ready`.
        let mut this = Some(self);

        let fut = futures::future::poll_fn(move |cx| match &mut this {
            Some(graph) => loop {
                match graph.nodes.poll_next_unpin(cx) {
                    Poll::Ready(_) => {}
                    Poll::Pending => return Poll::Ready(this.take().unwrap()),
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
        .multiplex(yamux::YamuxConfig::default())
        .boxed();

    let peer_id = public_key.to_peer_id();

    // NOTE: The graph of created nodes can be disconnected from the mesh point of view as nodes
    // can reach their d_lo value and not add other nodes to their mesh. To speed up this test, we
    // reduce the default values of the heartbeat, so that all nodes will receive gossip in a
    // timely fashion.

    let config = GossipsubConfigBuilder::default()
        .heartbeat_initial_delay(Duration::from_millis(100))
        .heartbeat_interval(Duration::from_millis(200))
        .history_length(10)
        .history_gossip(10)
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap();
    let behaviour = Gossipsub::new(MessageAuthenticity::Author(peer_id), config).unwrap();
    let mut swarm = Swarm::new(transport, behaviour, peer_id);

    let port = 1 + random::<u64>();
    let mut addr: Multiaddr = Protocol::Memory(port).into();
    swarm.listen_on(addr.clone()).unwrap();

    addr = addr.with(Protocol::P2p(public_key.to_peer_id().into()));

    (addr, swarm)
}

#[test]
fn multi_hop_propagation() {
    let _ = env_logger::try_init();

    fn prop(num_nodes: u8, seed: u64) -> TestResult {
        if !(2..=50).contains(&num_nodes) {
            return TestResult::discard();
        }

        debug!("number nodes: {:?}, seed: {:?}", num_nodes, seed);

        let mut graph = Graph::new_connected(num_nodes as usize, seed);
        let number_nodes = graph.nodes.len();

        // Subscribe each node to the same topic.
        let topic = Topic::new("test-net");
        for node in &mut graph.nodes {
            node.behaviour_mut().subscribe(&topic).unwrap();
        }

        // Wait for all nodes to be subscribed.
        let mut subscribed = 0;
        let all_subscribed = graph.wait_for(move |ev| {
            if let GossipsubEvent::Subscribed { .. } = ev {
                subscribed += 1;
                if subscribed == (number_nodes - 1) * 2 {
                    return true;
                }
            }

            false
        });
        if !all_subscribed {
            return TestResult::error(format!(
                "Timed out waiting for all nodes to subscribe but only have {:?}/{:?}.",
                subscribed, num_nodes,
            ));
        }

        // It can happen that the publish occurs before all grafts have completed causing this test
        // to fail. We drain all the poll messages before publishing.
        graph = graph.drain_poll();

        // Publish a single message.
        graph
            .nodes
            .iter_mut()
            .next()
            .unwrap()
            .behaviour_mut()
            .publish(topic, vec![1, 2, 3])
            .unwrap();

        // Wait for all nodes to receive the published message.
        let mut received_msgs = 0;
        let all_received = graph.wait_for(move |ev| {
            if let GossipsubEvent::Message { .. } = ev {
                received_msgs += 1;
                if received_msgs == number_nodes - 1 {
                    return true;
                }
            }

            false
        });
        if !all_received {
            return TestResult::error(format!(
                "Timed out waiting for all nodes to receive the msg but only have {:?}/{:?}.",
                received_msgs, num_nodes,
            ));
        }

        TestResult::passed()
    }

    QuickCheck::new()
        .max_tests(5)
        .quickcheck(prop as fn(u8, u64) -> TestResult)
}
