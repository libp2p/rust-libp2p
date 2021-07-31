//! A basic chat application demonstrating libp2p for use with async-await
//! through an actor-based design.
//!
//! The example is run per node as follows:
//!
//! ```sh
//! cargo run --example chat-async"
//! ```

use async_std::channel;
use async_std::io;
use async_std::prelude::*;
use futures::StreamExt;
use libp2p::{
    floodsub::{self, Floodsub, FloodsubEvent},
    identity,
    mdns::{Mdns, MdnsEvent},
    swarm::{SwarmBuilder, SwarmEvent},
    Multiaddr, NetworkBehaviour, PeerId, Swarm,
};
use std::error::Error;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    // Create a random PeerId
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().to_peer_id();

    // Create a Swarm to manage peers and events.
    let mut swarm = SwarmBuilder::new(
        libp2p::development_transport(id_keys).await?,
        MyBehaviour {
            floodsub: Floodsub::new(peer_id.clone()),
            mdns: Mdns::new(Default::default()).await?,
        },
        peer_id,
    )
    .build();

    // Reach out to another node if specified
    if let Some(to_dial) = std::env::args().nth(1) {
        let addr = to_dial.parse::<Multiaddr>()?;
        swarm.dial_addr(addr.clone())?;

        println!("Dialing {}", addr);
    }

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    let (out_msg_sender, out_msg_receiver) = channel::unbounded();
    let (in_msg_sender, in_msg_receiver) = channel::unbounded();

    // Spawn away the event loop that will keep the swarm going.
    async_std::task::spawn(network_event_loop(swarm, out_msg_receiver, in_msg_sender));

    // For demonstration purposes, we create a dedicated task that handles incoming messages.
    async_std::task::spawn(async move {
        let mut in_msg_receiver = in_msg_receiver.fuse();

        loop {
            let (peer, message) = in_msg_receiver.select_next_some().await;

            println!("{}: {}", peer, message);
        }
    });

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    // Reading from stdin represents our application.
    // Communication with the swarm happens via the channel so this could be anything really ...
    while let Some(Ok(line)) = stdin.next().await {
        out_msg_sender.send(line).await.unwrap();
    }

    Ok(())
}

/// Defines the event-loop of our application's network layer.
///
/// The event-loop handles some network events itself like mDNS and interacts with the rest
/// of the application via channels.
/// Conceptually, this is an actor-ish design.
async fn network_event_loop(
    mut swarm: Swarm<MyBehaviour>,
    receiver: channel::Receiver<String>,
    sender: channel::Sender<(PeerId, String)>,
) {
    // Create a Floodsub topic
    let chat = floodsub::Topic::new("chat");

    swarm.behaviour_mut().floodsub.subscribe(chat.clone());

    let mut receiver = receiver.fuse();

    loop {
        futures::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Listening on {}", address);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint,.. } => {
                        println!("Connected to {} on {}", peer_id, endpoint.get_remote_address());
                    }
                    SwarmEvent::ConnectionClosed { peer_id,.. } => {
                        println!("Disconnected from {}", peer_id);
                    }
                    SwarmEvent::Behaviour(MyOutEvent::Mdns(MdnsEvent::Discovered(list))) => {
                        for (peer, _) in list {
                            swarm.behaviour_mut().floodsub.add_node_to_partial_view(peer);
                        }
                    }
                    SwarmEvent::Behaviour(MyOutEvent::Mdns(MdnsEvent::Expired(list))) => {
                        for (peer, _) in list {
                            if !swarm.behaviour_mut().mdns.has_node(&peer) {
                                swarm.behaviour_mut().floodsub.remove_node_from_partial_view(&peer);
                            }
                        }
                    },
                    SwarmEvent::Behaviour(MyOutEvent::Floodsub(FloodsubEvent::Message(message))) if message.topics.contains(&chat) => {
                        let message_str = String::from_utf8(message.data).expect("bad message");

                        sender.send((message.source, message_str)).await.unwrap();
                    },
                    _ => {} // ignore all other events
                }
            },
            message = receiver.select_next_some() => {
                swarm.behaviour_mut().floodsub.publish(chat.clone(), message.as_bytes());
            }
        }
    }
}

// We create a custom network behaviour that combines floodsub and mDNS.
// The derive generates a delegating `NetworkBehaviour` implementation.
// We specify `event_process` false which - in combination with `out_event`,
// delegates all events upwards and allows us to process them in the event-loop.
#[derive(NetworkBehaviour)]
#[behaviour(event_process = false, out_event = "MyOutEvent")]
struct MyBehaviour {
    floodsub: Floodsub,
    mdns: Mdns,
}

#[derive(Debug)]
enum MyOutEvent {
    Floodsub(FloodsubEvent),
    Mdns(MdnsEvent),
}

impl From<FloodsubEvent> for MyOutEvent {
    fn from(event: FloodsubEvent) -> MyOutEvent {
        MyOutEvent::Floodsub(event)
    }
}

impl From<MdnsEvent> for MyOutEvent {
    fn from(event: MdnsEvent) -> MyOutEvent {
        MyOutEvent::Mdns(event)
    }
}
