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

//! A basic chat application demonstrating libp2p with the gossipsub protocol, supported by
//! the kademlia and identify protocols for peer discovery.
//! This example uses tokio for all asynchronous tasks and I/O. In order for all used libp2p
//! crates to use tokio, it enables tokio-specific features for some crates.
//!
//! The example is run per node as follows:
//!
//! ```sh
//! cargo run --features="tcp-tokio" --example peer-discovery
//! ```
//!
//! You can then connect additonal nodes by running the suggested command with the correct port and
//! peer id. For example:
//!
//! ```sh
//! cargo run -- /ip4/192.168.122.1/tcp/33839 12D3KooWNi5fDJByR7DVJh98oVETP5qKbCKxaAossfhzPAduUqWF
//! ```
//!
//! Because of the peer sharing powered by kademlia and identify, other nodes will remain connected
//! even when the initial node is shutdown.

use futures::StreamExt;
use libp2p::{
    core::{upgrade, PublicKey},
    gossipsub::{
        self, Gossipsub, GossipsubEvent, GossipsubMessage, IdentTopic, MessageAuthenticity,
        MessageId, ValidationMode,
    },
    identify::{Identify, IdentifyConfig, IdentifyEvent, IdentifyInfo},
    identity::{self, Keypair},
    kad::{
        self, record::store::MemoryStore, GetClosestPeersError, GetClosestPeersOk, Kademlia,
        KademliaConfig, KademliaEvent, QueryResult,
    },
    mplex, noise,
    swarm::NetworkBehaviourEventProcess,
    tcp::TokioTcpConfig,
    Multiaddr, NetworkBehaviour, PeerId, Swarm, Transport,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::{env::args, error::Error, time::Duration};
use tokio::io::{self, AsyncBufReadExt};

#[derive(NetworkBehaviour)]
#[behaviour(event_process = true)]
struct ChatBehaviour {
    gossipsub: Gossipsub,
    kademlia: Kademlia<MemoryStore>,
    identify: Identify,
}

impl NetworkBehaviourEventProcess<GossipsubEvent> for ChatBehaviour {
    fn inject_event(&mut self, event: GossipsubEvent) {
        if let GossipsubEvent::Message {
            propagation_source: peer_id,
            message_id: id,
            message,
        } = event
        {
            println!(
                "Got message: {} with id: {} from peer: {:?}",
                String::from_utf8_lossy(&message.data),
                id,
                peer_id
            );
        }
    }
}

impl NetworkBehaviourEventProcess<IdentifyEvent> for ChatBehaviour {
    fn inject_event(&mut self, event: IdentifyEvent) {
        match event {
            IdentifyEvent::Received {
                peer_id,
                info:
                    IdentifyInfo {
                        listen_addrs,
                        protocols,
                        ..
                    },
            } => {
                if protocols
                    .iter()
                    .any(|p| p.as_bytes() == kad::protocol::DEFAULT_PROTO_NAME)
                {
                    for addr in listen_addrs {
                        self.kademlia.add_address(&peer_id, addr);
                    }
                }
            }
            _ => {}
        }
    }
}

impl NetworkBehaviourEventProcess<KademliaEvent> for ChatBehaviour {
    fn inject_event(&mut self, _: KademliaEvent) {}
}

/// The `tokio::main` attribute sets up a tokio runtime.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    // Create a random PeerId
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    println!("Local peer id: {:?}", peer_id);

    // Create a keypair for authenticated encryption of the transport.
    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&id_keys)
        .expect("Signing libp2p-noise static DH keypair failed.");

    // Create a tokio-based TCP transport use noise for authenticated
    // encryption and Mplex for multiplexing of substreams on a TCP stream.
    let transport = TokioTcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(mplex::MplexConfig::new())
        .boxed();

    // Create a Gossipsub topic
    let gossipsub_topic = IdentTopic::new("chat");

    // get optional boot address and peerId
    let boot_addr: Option<Multiaddr> = args().nth(1).map(|a| a.parse().unwrap());
    let boot_peerid: Option<PeerId> = args().nth(2).map(|a| a.parse().unwrap());

    // Create a Swarm to manage peers and events.
    let mut swarm: Swarm<ChatBehaviour> = {
        let mut chatbehaviour = ChatBehaviour {
            gossipsub: create_gossipsub_behavior(id_keys.clone()),
            kademlia: create_kademlia_behavior(peer_id),
            identify: create_identify_behavior(id_keys.public()),
        };

        // subscribes to our topic
        chatbehaviour.gossipsub.subscribe(&gossipsub_topic).unwrap();

        libp2p::swarm::SwarmBuilder::new(transport, chatbehaviour, peer_id)
            // We want the connection background tasks to be spawned
            // onto the tokio runtime.
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build()
    };

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?).unwrap();

    // Reach out to another node if specified
    if let Some(boot_addr) = boot_addr {
        println!("adding boot node to kademlia routing table");
        swarm
            .behaviour_mut()
            .kademlia
            .add_address(&boot_peerid.unwrap(), boot_addr);
    }

    // Order Kademlia to search for a peer.
    let to_search: PeerId = identity::Keypair::generate_ed25519().public().into();
    println!("Searching for the closest peers to {:?}", to_search);
    swarm.behaviour_mut().kademlia.get_closest_peers(to_search);

    tokio::spawn(run(swarm, gossipsub_topic)).await.unwrap();
    Ok(())
}

async fn run(mut swarm: Swarm<ChatBehaviour>, gossipsub_topic: IdentTopic) {
    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines();
    // Kick it off
    let mut listening = false;
    loop {
        if !listening {
            for addr in swarm.listeners() {
                println!("Listening on {:?}", addr);
                let peer_id = swarm.local_peer_id();
                println!("connect additional nodes with \"cargo run --features=tcp-tokio --example peer-discovery -- {addr} {peer_id}\"");
                listening = true;
            }
        }
        let to_publish = {
            tokio::select! {
                line = stdin.next_line() => {
                    let line = line.unwrap().expect("stdin closed");
                    if !line.is_empty()  {
                        Some((gossipsub_topic.clone(), line))
                    } else {
                        println!("{}", line);
                        None
                    }
                }
                _ = swarm.select_next_some() => {
                    // All events are handled by the `NetworkBehaviourEventProcess`es.
                    // I.e. the `swarm.select_next_some()` future drives the `Swarm` without ever
                    // terminating.
                    None
                }
            }
        };
        if let Some((topic, line)) = to_publish {
            swarm
                .behaviour_mut()
                .gossipsub
                .publish(topic.clone(), line.as_bytes())
                .unwrap();
        }
    }
}

fn create_gossipsub_behavior(id_keys: Keypair) -> Gossipsub {
    // To content-address message, we can take the hash of message and use it as an ID.
    let message_id_fn = |message: &GossipsubMessage| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        MessageId::from(s.finish().to_string())
    };

    // Set a custom gossipsub
    let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
        .validation_mode(ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
        .message_id_fn(message_id_fn) // content-address messages. No two messages of the
        .do_px()
        // same content will be propagated.
        .build()
        .expect("Valid config");
    gossipsub::Gossipsub::new(MessageAuthenticity::Signed(id_keys), gossipsub_config)
        .expect("Correct configuration")
}

fn create_kademlia_behavior(local_peer_id: PeerId) -> Kademlia<MemoryStore> {
    // Create a Kademlia behaviour.
    let mut cfg = KademliaConfig::default();
    cfg.set_query_timeout(Duration::from_secs(5 * 60));
    let store = MemoryStore::new(local_peer_id);
    Kademlia::with_config(local_peer_id, store, cfg)
}

fn create_identify_behavior(local_public_key: PublicKey) -> Identify {
    Identify::new(IdentifyConfig::new("/ipfs/1.0.0".into(), local_public_key))
}
