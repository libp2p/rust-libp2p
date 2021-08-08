#![allow(dead_code, unused_variables)]

// Based on the following example code:
// https://github.com/zupzup/rust-peer-to-peer-example/blob/main/src/main.rs

use libp2p::{
    core::upgrade,
    identity,
    kad::{record::store::MemoryStore, Kademlia, KademliaEvent, QueryResult},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    mplex,
    noise::{Keypair, NoiseConfig, X25519Spec},
    swarm::NetworkBehaviourEventProcess,
    tcp::TokioTcpConfig,
    NetworkBehaviour, PeerId, Swarm, Transport,
};

use futures::prelude::*;
use std::task::{Context, Poll};
use tokio::runtime::Runtime;

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    kademlia: Kademlia<MemoryStore>,
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
                    println!("{}", &peer_id);
                    self.kademlia.add_address(&peer_id, multiaddr);
                }
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let key = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from_public_key(key.public());

    let auth_keys = Keypair::<X25519Spec>::new() // create new auth keys
        .into_authentic(&key) // sign the keys
        .unwrap();

    let transport = TokioTcpConfig::new()
        .upgrade(upgrade::Version::V1) // upgrade will only show up if you import `Transport`
        .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())
        .multiplex(mplex::MplexConfig::new())
        .boxed();

    let behaviour = MyBehaviour {
        kademlia: Kademlia::new(peer_id.clone(), MemoryStore::new(peer_id.clone())),
        mdns: Mdns::new(MdnsConfig::default()).await.unwrap(),
    };

    let mut swarm = Swarm::new(transport, behaviour, peer_id.clone());

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    loop {
        let mut buffer = String::new();
        println!("reading");
        std::io::stdin().read_line(&mut buffer).unwrap();

        if buffer.starts_with("lp") {
            println!("printing peers");
            for peer in swarm.behaviour_mut().mdns.discovered_nodes() {
                println!("{}", peer);
            }
        }
    }
}
