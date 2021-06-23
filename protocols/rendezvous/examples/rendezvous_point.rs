use futures::StreamExt;
use libp2p::core::identity;
use libp2p::core::PeerId;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::rendezvous::Rendezvous;
use libp2p::swarm::Swarm;
use libp2p::NetworkBehaviour;
use libp2p::{development_transport, rendezvous};

#[tokio::main]
async fn main() {
    let bytes = [0u8; 32];
    let key = identity::ed25519::SecretKey::from_bytes(bytes).expect("we always pass 32 bytes");
    let identity = identity::Keypair::Ed25519(key.into());

    let mut swarm = Swarm::new(
        development_transport(identity.clone()).await.unwrap(),
        MyBehaviour {
            identify: Identify::new(IdentifyConfig::new(
                "rendezvous-example/1.0.0".to_string(),
                identity.public(),
            )),
            rendezvous: Rendezvous::new(identity.clone(), 10000),
        },
        PeerId::from(identity.public()),
    );

    println!("peer id: {}", swarm.local_peer_id());

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/62649".parse().unwrap())
        .unwrap();

    while let Some(event) = swarm.next().await {
        println!("{:?}", event);
    }
}

#[derive(Debug)]
enum MyEvent {
    Rendezvous(rendezvous::Event),
    Identify(IdentifyEvent),
}

impl From<rendezvous::Event> for MyEvent {
    fn from(event: rendezvous::Event) -> Self {
        MyEvent::Rendezvous(event)
    }
}

impl From<IdentifyEvent> for MyEvent {
    fn from(event: IdentifyEvent) -> Self {
        MyEvent::Identify(event)
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = false)]
#[behaviour(out_event = "MyEvent")]
struct MyBehaviour {
    identify: Identify,
    rendezvous: Rendezvous,
}
