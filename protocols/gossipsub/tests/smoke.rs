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

use std::{task::Poll, time::Duration};

use futures::{
    stream::{FuturesUnordered, SelectAll},
    StreamExt,
};
use libp2p_gossipsub as gossipsub;
use libp2p_gossipsub::{MessageAuthenticity, ValidationMode};
use libp2p_swarm::Swarm;
use libp2p_swarm_test::SwarmExt as _;
use quickcheck::{QuickCheck, TestResult};
use rand::{seq::SliceRandom, SeedableRng};
use tokio::{runtime::Runtime, time};
use tracing_subscriber::EnvFilter;

struct Graph {
    nodes: SelectAll<Swarm<gossipsub::Behaviour>>,
}

impl Graph {
    async fn new_connected(num_nodes: usize, seed: u64) -> Graph {
        if num_nodes == 0 {
            panic!("expecting at least one node");
        }

        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let mut not_connected_nodes = (0..num_nodes)
            .map(|_| build_node())
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await;

        let mut connected_nodes = vec![not_connected_nodes.pop().unwrap()];

        for mut next in not_connected_nodes {
            let connected = connected_nodes
                .choose_mut(&mut rng)
                .expect("at least one connected node");

            next.connect(connected).await;

            connected_nodes.push(next);
        }

        Graph {
            nodes: SelectAll::from_iter(connected_nodes),
        }
    }

    /// Polls the graph and passes each event into the provided FnMut until the closure returns
    /// `true`.
    ///
    /// Returns [`true`] on success and [`false`] on timeout.
    async fn wait_for<F: FnMut(&gossipsub::Event) -> bool>(&mut self, mut f: F) -> bool {
        let condition = async {
            loop {
                if let Ok(ev) = self
                    .nodes
                    .select_next_some()
                    .await
                    .try_into_behaviour_event()
                {
                    if f(&ev) {
                        break;
                    }
                }
            }
        };

        match time::timeout(Duration::from_secs(10), condition).await {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// Polls the graph until Poll::Pending is obtained, completing the underlying polls.
    async fn drain_events(&mut self) {
        let fut = futures::future::poll_fn(|cx| loop {
            match self.nodes.poll_next_unpin(cx) {
                Poll::Ready(_) => {}
                Poll::Pending => return Poll::Ready(()),
            }
        });
        time::timeout(Duration::from_secs(10), fut).await.unwrap();
    }
}

async fn build_node() -> Swarm<gossipsub::Behaviour> {
    // NOTE: The graph of created nodes can be disconnected from the mesh point of view as nodes
    // can reach their d_lo value and not add other nodes to their mesh. To speed up this test, we
    // reduce the default values of the heartbeat, so that all nodes will receive gossip in a
    // timely fashion.

    let mut swarm = Swarm::new_ephemeral(|identity| {
        let peer_id = identity.public().to_peer_id();

        let config = gossipsub::ConfigBuilder::default()
            .heartbeat_initial_delay(Duration::from_millis(100))
            .heartbeat_interval(Duration::from_millis(200))
            .history_length(10)
            .history_gossip(10)
            .validation_mode(ValidationMode::Permissive)
            .build()
            .unwrap();
        gossipsub::Behaviour::new(MessageAuthenticity::Author(peer_id), config).unwrap()
    });
    swarm.listen().with_memory_addr_external().await;

    swarm
}

#[test]
fn multi_hop_propagation() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    fn prop(num_nodes: u8, seed: u64) -> TestResult {
        if !(2..=50).contains(&num_nodes) {
            return TestResult::discard();
        }

        tracing::debug!(number_of_nodes=%num_nodes, seed=%seed);

        let rt = Runtime::new().unwrap();

        rt.block_on(async move {
            let mut graph = Graph::new_connected(num_nodes as usize, seed).await;
            let number_nodes = graph.nodes.len();

            // Subscribe each node to the same topic.
            let topic = gossipsub::IdentTopic::new("test-net");
            for node in &mut graph.nodes {
                node.behaviour_mut().subscribe(&topic).unwrap();
            }

            // Wait for all nodes to be subscribed.
            let mut subscribed = 0;

            let all_subscribed = graph
                .wait_for(move |ev| {
                    if let gossipsub::Event::Subscribed { .. } = ev {
                        subscribed += 1;
                        if subscribed == (number_nodes - 1) * 2 {
                            return true;
                        }
                    }

                    false
                })
                .await;

            if !all_subscribed {
                return TestResult::error(format!(
                    "Timed out waiting for all nodes to subscribe but only have {subscribed:?}/{num_nodes:?}.",
                ));
            }

            // It can happen that the publish occurs before all grafts have completed causing this test
            // to fail. We drain all the poll messages before publishing.
            graph.drain_events().await;

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
            let all_received = graph
                .wait_for(move |ev| {
                    if let gossipsub::Event::Message { .. } = ev {
                        received_msgs += 1;
                        if received_msgs == number_nodes - 1 {
                            return true;
                        }
                    }

                    false
                })
                .await;

            if !all_received {
                return TestResult::error(format!(
                    "Timed out waiting for all nodes to receive the msg but only have {received_msgs:?}/{num_nodes:?}.",
                ));
            }

            TestResult::passed()
        })
    }

    QuickCheck::new()
        .max_tests(5)
        .quickcheck(prop as fn(u8, u64) -> TestResult)
}
