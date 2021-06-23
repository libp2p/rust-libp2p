use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::{SelectUpgrade, Version};
use libp2p::core::PeerId;
use libp2p::core::{identity, Transport};
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::mplex::MplexConfig;
use libp2p::noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p::rendezvous;
use libp2p::rendezvous::Rendezvous;
use libp2p::swarm::Swarm;
use libp2p::tcp::TcpConfig;
use libp2p::yamux::YamuxConfig;
use libp2p::NetworkBehaviour;
use std::time::Duration;

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

#[tokio::main]
async fn main() {
    let bytes = [0u8; 32];
    let key = identity::ed25519::SecretKey::from_bytes(bytes).expect("we always pass 32 bytes");
    let identity = identity::Keypair::Ed25519(key.into());

    let peer_id = PeerId::from(identity.public());

    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&identity)
        .expect("failed to create dh_keys");

    let transport = TcpConfig::new()
        .upgrade(Version::V1)
        .authenticate(NoiseConfig::xx(dh_keys).into_authenticated())
        .multiplex(SelectUpgrade::new(
            YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let identify = Identify::new(IdentifyConfig::new(
        "rendezvous-example/1.0.0".to_string(),
        identity.public(),
    ));
    let rendezvous = Rendezvous::new(identity, 10000);

    let mut swarm = Swarm::new(
        transport,
        MyBehaviour {
            identify,
            rendezvous,
        },
        peer_id,
    );

    println!("peer id: {}", swarm.local_peer_id());

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/62649".parse().unwrap())
        .unwrap();

    loop {
        let event = swarm.next().await;
        println!("swarm event: {:?}", event);
    }
}
