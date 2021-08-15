#![allow(dead_code, unused_variables)]

// Use the following command to run:
// cargo run --example kademlia-example --features="tcp-tokio mdns"
// To run in client mode:
// cargo run --example kademlia-example --features="tcp-tokio mdns" --client

// Based on the following example code:
// https://github.com/zupzup/rust-peer-to-peer-example/blob/main/src/main.rs

use libp2p::{
    core::upgrade,
    identity,
    kad::{
        protocol::Mode, record::store::MemoryStore, Kademlia, KademliaConfig, KademliaEvent,
        QueryResult,
    },
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    mplex,
    noise::{Keypair, NoiseConfig, X25519Spec},
    swarm::SwarmEvent,
    tcp::TokioTcpConfig,
    NetworkBehaviour, PeerId, Swarm, Transport,
};

use async_std::io;
use futures::{FutureExt, StreamExt};
use std::env;

#[derive(NetworkBehaviour)]
#[behaviour(event_process = false, out_event = "MyOutEvent")]
struct MyBehaviour {
    kademlia: Kademlia<MemoryStore>,
    // Using MDNS for peer discovery.
    mdns: Mdns,
}

enum MyOutEvent {
    Kademlia(KademliaEvent),
    Mdns(MdnsEvent),
}

impl From<KademliaEvent> for MyOutEvent {
    fn from(event: KademliaEvent) -> Self {
        MyOutEvent::Kademlia(event)
    }
}

impl From<MdnsEvent> for MyOutEvent {
    fn from(event: MdnsEvent) -> Self {
        MyOutEvent::Mdns(event)
    }
}

#[tokio::main]
async fn main() {
    let client_mode: bool = if let Some(kad_mode) = env::args().nth(1) {
        kad_mode.starts_with("--client")
    } else {
        false
    };

    create_peer(client_mode).await
}

async fn create_peer(client_mode: bool) {
    let key = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from_public_key(&key.public());

    // Print the current peer ID.
    println!("{:?}", peer_id.clone());

    let auth_keys = Keypair::<X25519Spec>::new() // create new auth keys
        .into_authentic(&key) // sign the keys
        .unwrap();

    let transport = TokioTcpConfig::new()
        .upgrade(upgrade::Version::V1) // upgrade will only show up if you import `Transport`
        .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())
        .multiplex(mplex::MplexConfig::new())
        .boxed();

    let mut config = KademliaConfig::default();

    let kad_config = if client_mode {
        config.set_mode(Mode::Client);
        println!("Setting to client mode.");
        config
    } else {
        config
    };

    let behaviour = MyBehaviour {
        kademlia: Kademlia::with_config(
            peer_id.clone(),
            MemoryStore::new(peer_id.clone()),
            kad_config,
        ),
        mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
    };

    let mut swarm = Swarm::new(transport, behaviour, peer_id.clone());

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    let stdin = io::stdin();
    let mut buffer = String::new();

    loop {
        futures::select! {
            line = stdin.read_line(&mut buffer).fuse() => handle_command(buffer.clone(), &mut swarm),
            event = swarm.next() => match event.unwrap() {
                SwarmEvent::NewListenAddr { address, .. } => println!("Listening on: {}", address),
                SwarmEvent::Behaviour(event) => handle_event(event, &mut swarm),
                _ => {}
            }
        }
    }
}

fn handle_command(command: String, swarm: &mut Swarm<MyBehaviour>) {
    if command.contains("list peer") {
        list_peers(swarm);
    }
}

fn handle_event(event: MyOutEvent, swarm: &mut Swarm<MyBehaviour>) {
    match event {
        MyOutEvent::Kademlia(kad_event) => match kad_event {
            KademliaEvent::OutboundQueryCompleted { id, result, stats } => todo!(),
            _ => {}
        },
        MyOutEvent::Mdns(mdns_event) => match mdns_event {
            MdnsEvent::Discovered(nodes) => {
                for (peer_id, multiaddr) in nodes {
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, multiaddr);
                }
            }
            _ => {}
        },
    }
}

fn list_peers(swarm: &mut Swarm<MyBehaviour>) {
    for bucket in swarm.behaviour_mut().kademlia.kbuckets() {
        if bucket.num_entries() > 0 {
            for item in bucket.iter() {
                println!("Peer ID: {:?}", item.node.key.preimage());
            }
        }
    }
}
