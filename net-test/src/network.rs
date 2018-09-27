// Copyright 2018 Parity Technologies (UK) Ltd.
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

use std::{
    collections::HashMap,
    fmt,
    io,
    ops::RangeInclusive,
    sync::mpsc as sync_mpsc,
    time::Duration,
};
use futures::{prelude::*, stream, sync::mpsc};
use libp2p::Multiaddr;
use libp2p::secio::SecioKeyPair;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::nodes::handled_node::NodeHandler;
use libp2p::core::nodes::node::Substream;
use libp2p::core::nodes::swarm::{Swarm, SwarmEvent};
use rand;
use tokio_core::reactor::Core;
use tokio::runtime::Runtime;
use netsim::{self, spawn, node, node::Ipv4Node, Ipv4Range};
use transport;

/// Identifier for a node that is part of the network.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(String);

impl fmt::Display for NodeId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

/// Prototype for a network. Contains all the pending nodes and connections.
/// Usage: fill it with `node` and `connect`, then run `start` to start it.
pub struct Network {
    /// List of nodes to spawn.
    /// The function is responsible for running the node to completion.
    /// The function parameters are the address to listen on, a sender to notify when we are
    /// initialized with the actual listen address, and a receiver for addresses to dial.
    nodes: HashMap<NodeId, Box<FnMut(Multiaddr, sync_mpsc::Sender<Multiaddr>, mpsc::UnboundedReceiver<Multiaddr>) + Send>>,
    /// Connections to establish.
    connections: Vec<(NodeId, NodeId)>,
    /// Timeout for establishing a connection with a remote.
    transport_timeout: Option<Duration>,
    /// Possible range of hops.
    hops_range: RangeInclusive<u32>,
    /// Min latency.
    min_latency: Duration,
    /// Possible additional latency.
    mean_additional_latency: Duration,
    /// Packet loss rate.
    packet_loss_rate: f64,
    /// Mean duration of a packet loss.
    mean_packet_loss_duration: Duration,
}

impl Default for Network {
    fn default() -> Network {
        Network {
            nodes: Default::default(),
            connections: Vec::new(),
            transport_timeout: Some(Duration::from_secs(30)),
            hops_range: 6 ..= 12,
            min_latency: Duration::from_millis(100),
            mean_additional_latency: Duration::from_millis(500),
            packet_loss_rate: 0.05,
            mean_packet_loss_duration: Duration::from_millis(900),
        }
    }
}

impl Network {
    /// Add a new node to the network.
    pub fn node<THandler, TName, TBuilder>(&mut self, id: TName, handler: TBuilder) -> NodeId
    where TName: Into<String>,
          TBuilder: Fn() -> THandler + Send + 'static,
          THandler: NodeHandler<Substream<StreamMuxerBox>, InEvent = (), OutEvent = ()> + Send + 'static,
          THandler::OutboundOpenInfo: Send + 'static,
    {
        let id = NodeId(id.into());
        let id2 = id.clone();

        // This closure is responsible for running a node to completion.
        let transport_timeout = self.transport_timeout;
        let node = move |listen_addr, init_tx: sync_mpsc::Sender<Multiaddr>, conn_rx: mpsc::UnboundedReceiver<Multiaddr>| {
            let key = SecioKeyPair::ed25519_generated().unwrap();
            let mut swarm = Swarm::with_handler_builder(
                transport::build_transport(key.clone(), transport_timeout),
                handler
            );

            let new_addr = swarm.listen_on(listen_addr).unwrap();
            init_tx.send(new_addr).expect("Network not running");

            let mut conn_rx = conn_rx.fuse();
            let future = stream::poll_fn(move || -> Poll<_, io::Error> {
                loop {
                    match conn_rx.poll() {
                        Ok(Async::Ready(Some(dial_addr))) => swarm.dial(dial_addr).unwrap(),
                        Ok(Async::NotReady) => break,
                        Ok(Async::Ready(None)) | Err(_) => return Ok(Async::Ready(None)),
                    }
                }

                loop {
                    match swarm.poll() {
                        Async::NotReady => return Ok(Async::NotReady),
                        Async::Ready(None) => return Ok(Async::Ready(None)),
                        Async::Ready(Some(ev)) => match ev {
                            SwarmEvent::IncomingConnection { .. } => {},
                            SwarmEvent::IncomingConnectionError { error, .. } => {
                                panic!("Incoming connection error: {:?}", error);
                            },
                            SwarmEvent::Connected { .. } => {},
                            SwarmEvent::ListenerClosed { result, .. } => {
                                panic!("Listener closed: {:?}", result);
                            },
                            SwarmEvent::Replaced { .. } => {},
                            SwarmEvent::NodeClosed { .. } => {
                                if !swarm.has_connections_or_pending() {
                                    // We send all the dial requests before starting to poll this
                                    // future, therefore the swarm will be empty only when all
                                    // nodes are done.
                                    return Ok(Async::Ready(None));
                                }
                            },
                            SwarmEvent::NodeError { peer_id, error, .. } => {
                                panic!("{:?} NodeError: {:?}", peer_id, error);
                            },
                            SwarmEvent::DialError { peer_id, error, .. } => {
                                panic!("{:?} DialError: {:?}", peer_id, error);
                            },
                            SwarmEvent::UnknownPeerDialError { error, .. } => {
                                panic!("UnknownPeerDialError: {:?}", error);
                            },
                            _ => {
                            },
                        }
                    }
                }
            }).for_each(|_: Option<()>| Ok(()));

            let runtime = Runtime::new().unwrap();
            runtime.block_on_all(future).unwrap();
        };

        // Insert into the list of nodes.
        let runner = {
            let mut node = Some(node);
            Box::new(move |a, b, c| { let f = node.take().unwrap(); f(a, b, c) }) as Box<_>
        };
        let prev = self.nodes.insert(id2.clone(), runner);
        assert!(prev.is_none(), "Duplicate id: {}", id2);
        id2
    }

    /// Configures the possible number of hops between two connections.
    #[inline]
    pub fn hops_range(&mut self, range: impl Into<RangeInclusive<u32>>) {
        self.hops_range = range.into();
    }

    /// Connects all nodes between each other.
    pub fn connect_all(&mut self, nodes: &[NodeId]) {
        for (offset, node_a) in nodes.iter().enumerate() {
            for node_b in nodes.iter().skip(offset + 1) {
                self.connections.push((node_a.clone(), node_b.clone()));
            }
        }
    }

    /// Start the network.
    pub fn start(self) {
        let mut core = Core::new().unwrap();
        let network = netsim::Network::new(&core.handle());

        let mut nodes = vec![];
        let mut addresses = HashMap::new();
        let mut connect_to = HashMap::new();

        for (id, mut node) in self.nodes {
            let (conn_tx, conn_rx) = mpsc::unbounded();
            let (init_tx, init_rx) = sync_mpsc::channel();
            addresses.insert(id.clone(), init_rx);
            connect_to.insert(id.clone(), conn_tx);

            nodes.push(node::ipv4::machine(move |addr| {
                let multiaddr = multiaddr![Ip4(addr), Tcp(1025u16)];
                node(multiaddr, init_tx, conn_rx);
            }));
        }

        let hops = if *self.hops_range.end() <= *self.hops_range.start() {
            *self.hops_range.start()
        } else {
            let len = *self.hops_range.end() - *self.hops_range.start();
            *self.hops_range.start() + rand::random::<u32>() % len
        };

        // Connect the sending and receiving nodes via a router
        let router_node = node::ipv4::router(nodes)
            .hops(hops)
            .latency(self.min_latency, self.mean_additional_latency)
            .packet_loss(self.packet_loss_rate, self.mean_packet_loss_duration);

        // Run the network with the router as the top-most node. `_plug` could be used send/receive
        // packets from/to outside the network
        let (spawn_complete, _plug) = spawn::ipv4_tree(&network.handle(), Ipv4Range::global(), router_node);

        // Make sure we collect the addresses and connect to each other.
        let mut addr = HashMap::new();
        for (id, init_rx) in addresses {
            match init_rx.recv() {
                Ok(a) => {
                    addr.insert(id, a);
                },
                Err(e) => {
                    println!("Unable to get address of {}: {:?}", id, e);
                },
            }
        }

        // connect to each other
        for (a, b) in self.connections {
            match (connect_to.get(&a), addr.get(&b)) {
                (Some(tx), Some(addr)) => {
                    tx.unbounded_send(addr.clone()).expect("Node should be listening for connections.");
                },
                _ => {},
            }
        }

        // Drive the network on the event loop.
        core.run(spawn_complete).unwrap();
    }
}
