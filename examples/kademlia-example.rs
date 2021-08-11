#![allow(dead_code, unused_variables)]

// Use the following command to run:
// cargo run --example kademlia-example --features="tcp-tokio mdns"

// Based on the following example code:
// https://github.com/zupzup/rust-peer-to-peer-example/blob/main/src/main.rs

use libp2p::{
    core::{either::EitherError, upgrade},
    identity,
    kad::{record::store::MemoryStore, Kademlia, KademliaConfig, KademliaEvent, QueryResult},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    mplex,
    noise::{Keypair, NoiseConfig, X25519Spec},
    swarm::{NetworkBehaviourEventProcess, SwarmEvent},
    tcp::TokioTcpConfig,
    NetworkBehaviour, PeerId, Swarm, Transport,
};

use async_std::io;
use futures::{FutureExt, StreamExt};
use std::env;

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    kademlia: Kademlia<MemoryStore>,
    // Using MDNS for peer discovery.
    mdns: Mdns,
}

impl NetworkBehaviourEventProcess<KademliaEvent> for MyBehaviour {
    fn inject_event(&mut self, event: KademliaEvent) {
        match event {
            KademliaEvent::OutboundQueryCompleted { id, result, stats } => match result {
                QueryResult::GetClosestPeers(Ok(ok)) => {}
                _ => {}
            },
            _ => {}
        }
    }
}

impl NetworkBehaviourEventProcess<MdnsEvent> for MyBehaviour {
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(nodes) => {
                for (peer_id, multiaddr) in nodes {
                    self.kademlia.add_address(&peer_id, multiaddr);
                }
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let client_mode = if let Some(kad_mode) = env::args().nth(1) {
        true
    } else {
        false
    };
    create_peer(client_mode)
}

async fn create_peer(client_mode: bool) {
    let key = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from_public_key(&key.public());

    let auth_keys = Keypair::<X25519Spec>::new() // create new auth keys
        .into_authentic(&key) // sign the keys
        .unwrap();

    let transport = TokioTcpConfig::new()
        .upgrade(upgrade::Version::V1) // upgrade will only show up if you import `Transport`
        .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())
        .multiplex(mplex::MplexConfig::new())
        .boxed();

    let kad_config = if client_mode {
        KademliaConfig::default()
    } else {
        KademliaConfig::default()
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

    let mut stdin = io::stdin();
    let mut buffer = String::new();

    loop {
        futures::select! {
            line = stdin.read_line(&mut buffer).fuse() => handle_command(buffer),
            event = swarm.next() => handle_event(event.unwrap()),
        }
    }
}

fn handle_command(command: String, swarm: &Swarm<MyBehaviour>) {
    if command.contains("list peer") {
        list_peers(&swarm);
    }
}

fn handle_event(event: SwarmEvent<(), EitherError>) {
    match event {
        SwarmEvent::NewListenAddr { address, .. } => println!("Listening on: {}", address),
        _ => {}
    }
}

fn list_peers(swarm: &Swarm<MyBehaviour>) {
    for bucket in swarm.behaviour_mut().kademlia.kbuckets() {
        if bucket.num_entries() > 0 {
            for item in bucket.iter() {
                println!("Peer ID: {:?}", item.node.key);
            }
        }
    }
}
