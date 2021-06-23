use futures::StreamExt;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::NetworkBehaviour;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::{SelectUpgrade, Version};
use libp2p_core::PeerId;
use libp2p_core::{identity, Transport};
use libp2p_mplex::MplexConfig;
use libp2p_noise::{Keypair, X25519Spec};
use libp2p_rendezvous::{Event as RendezvousEvent, Rendezvous};
use libp2p_swarm::Swarm;
use libp2p_tcp::TcpConfig;
use libp2p_yamux::YamuxConfig;
use std::time::Duration;

#[derive(Debug)]
enum MyEvent {
    Rendezvous(RendezvousEvent),
    Identify(IdentifyEvent),
}

impl From<RendezvousEvent> for MyEvent {
    fn from(event: RendezvousEvent) -> Self {
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
    let noise_config = libp2p_noise::NoiseConfig::xx(dh_keys).into_authenticated();

    let tcp_config = TcpConfig::new();
    let transport = tcp_config
        .upgrade(Version::V1)
        .authenticate(noise_config)
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
