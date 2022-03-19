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

//! A basic chat application with logs demonstrating libp2p and the gossipsub protocol.
//!
//! Using two terminal windows, start two instances. Type a message in either terminal and hit return: the
//! message is sent and printed in the other terminal. Close with Ctrl-c.
//!
//! You can of course open more terminal windows and add more participants.
//! Dialing any of the other peers will propagate the new participant to all
//! chat members and everyone will receive all messages.
//!
//! In order to get the nodes to connect, take note of the listening addresses of the first
//! instance and start the second with one of the addresses as the first argument. In the first
//! terminal window, run:
//!
//! ```sh
//! cargo run --example gossipsub-chat
//! ```
//!
//! It will print the [`PeerId`] and the listening addresses, e.g. `Listening on
//! "/ip4/0.0.0.0/tcp/24915"`
//!
//! In the second terminal window, start a new instance of the example with:
//!
//! ```sh
//! cargo run --example gossipsub-chat -- /ip4/127.0.0.1/tcp/24915
//! ```
//!
//! The two nodes should then connect.
use env_logger::{Builder, Env};
use libp2p::core::upgrade;
use libp2p::gossipsub::MessageId;
use libp2p::gossipsub::{
    GossipsubEvent, GossipsubMessage, IdentTopic as Topic, MessageAuthenticity, ValidationMode,
};
use libp2p::NetworkBehaviour;
use libp2p::Transport;
use libp2p::{gossipsub, identity, swarm::SwarmEvent, Multiaddr, PeerId};
use libp2p_mdns::{Mdns, MdnsConfig, MdnsEvent};
use libp2p_swarm::NetworkBehaviourEventProcess;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::io::{self, AsyncBufReadExt};
use tokio_stream::StreamExt as _;

#[derive(NetworkBehaviour)]
#[behaviour(event_process = true)]
struct Behaviour {
    gossipsub: gossipsub::Gossipsub,
    mdns: Mdns,
    #[behaviour(ignore)]
    sender: tokio::sync::mpsc::Sender<PeerId>,
    #[behaviour(ignore)]
    peers: HashMap<PeerId, Multiaddr>,
}

impl NetworkBehaviourEventProcess<MdnsEvent> for Behaviour {
    // Called when `mdns` produces an event.
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(list) => {
                use libp2p_swarm::NetworkBehaviour;

                for (peer_id, _) in list {
                    if !self.peers.contains_key(&peer_id) {
                        match self.sender.try_send(peer_id) {
                            Ok(_) => {
                                let address = self.addresses_of_peer(&peer_id).first().unwrap();
                                self.peers.insert(peer_id, address);
                            }
                            Err(_) => eprintln!("Failed to send."),
                        }
                    }

                    // Or you can just add an explicit peer.
                    // Note: This peer will not be in the mesh.
                    // This will lead to the IdleTimeout that will disconnect an explicit peer if this peer didn't send or didn't accept a message for the specific time.
                    // You can change IdleTimout timer via `GossipsubConfig` with the method `idle_timeout()`.
                    // self.gossipsub.add_explicit_peer(&peer_id); <-- adding an explicit peer.
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer_id, address) in list {
                    if self.peers.contains_key(&peer_id) && !self.mdns.has_node(&peer_id) {
                        println!("Expired Peer | {:?} | {:?}", peer_id, address);
                        self.peers.remove(&peer_id);
                    }
                }
            }
        }
    }
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for Behaviour {
    fn inject_event(&mut self, event: GossipsubEvent) {
        if let GossipsubEvent::Message {
            propagation_source: peer,
            message: msg,
            ..
        } = event
        {
            println!(
                "Got message from Peer | {:?} | {:?}",
                peer,
                String::from_utf8_lossy(&msg.data)
            );
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    // Create a random PeerId
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    let noise_keys = libp2p::noise::Keypair::<libp2p::noise::X25519Spec>::new()
        .into_authentic(&local_key)
        .expect("Signing libp2p-noise static DH keypair failed.");

    // Set up an encrypted TCP Transport over the Mplex and Yamux protocols
    let transport = libp2p::tcp::TokioTcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        // Note: Only the XX handshake pattern is currently guaranteed to provide interoperability with other libp2p implementations.
        .authenticate(libp2p::noise::NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(libp2p::mplex::MplexConfig::new())
        .boxed();

    // Create a Gossipsub topic
    let topic = Topic::new("test-net");

    let (sender, mut receiver) =
        tokio::sync::mpsc::channel::<PeerId>(std::mem::size_of::<PeerId>());

    // Create a Swarm to manage peers and events
    let mut swarm = {
        // To content-address message, we can take the hash of message and use it as an ID.
        let message_id_fn = |message: &GossipsubMessage| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            MessageId::from(s.finish().to_string())
        };

        // Set a custom gossipsub
        let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10)) //  Heartbeat sends empty messages to insure that connect is alive.
            .validation_mode(ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
            .message_id_fn(message_id_fn) // Content-address messages. No two messages of the tame content will be propagated.
            .idle_timeout(Duration::from_secs(5 * 60)) // IdleTimeout exists for the explicit peers that aren't in the mesh.
            // If the peer doesn't in the mesh or doesn't accept/send the messages for a specific time (IdleTimeout),
            // It will be disconnected from the peer.
            .build()
            .expect("Failed to build a GossipsubConfig");
        // build a gossipsub network behaviour
        let mut gossipsub: gossipsub::Gossipsub =
            gossipsub::Gossipsub::new(MessageAuthenticity::Signed(local_key), gossipsub_config)
                .expect("Correct configuration");

        // subscribes to our topic
        gossipsub.subscribe(&topic).unwrap();

        let behaviour = Behaviour {
            gossipsub,
            mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
            sender,
            peers: Default::default(),
        };

        // build the swarm
        libp2p::swarm::SwarmBuilder::new(transport, behaviour, local_peer_id)
            .executor(Box::new(|future| {
                tokio::spawn(future);
            }))
            .build()
    };

    // Listen on all interfaces and whatever port the OS assigns
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    // Reach out to another node if specified
    if let Some(to_dial) = std::env::args().nth(1) {
        let address: Multiaddr = to_dial
            .parse()
            .expect("Failed to parse provided `Multiaddr`.");
        match swarm.dial(address.clone()) {
            Ok(_) => println!("Dialed {:?}.", address),
            Err(e) => println!("Dial {:?} failed: {:?}", address, e),
        }
    }

    // Read full lines from stdin
    let mut stdin =
        tokio_stream::wrappers::LinesStream::new(io::BufReader::new(io::stdin()).lines()).fuse();

    // Kick it off
    loop {
        tokio::select! {
            Some(line) = stdin.next() => {
                if let Err(e) = swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(topic.clone(), line.expect("Stdin not to close").as_bytes())
                {
                    println!("Publish error: {:?}", e);
                }
            },
            Some(event) = tokio_stream::StreamExt::next(&mut swarm) => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Now Listening | {:?}", address);
                }
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    ..
                } => {
                    println!("Established Connection with Peer | {:?}", peer_id);
                }
                _ => (),
            },
            Some(peer_id) = receiver.recv() => {
                if let Some(address) = swarm
                    .behaviour()
                    .peers
                    .get(&peer_id)
                    .cloned()
                {
                    if let Err(e) = swarm.dial(address) {
                        eprintln!("Failed to dial: {:?}", e);
                    }
                }
            }
        }
    }
}
