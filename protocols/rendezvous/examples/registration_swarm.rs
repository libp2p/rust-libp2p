use async_std::task;
use futures::StreamExt;
use libp2p::NetworkBehaviour;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::{SelectUpgrade, Version};
use libp2p_core::PeerId;
use libp2p_core::{identity, Transport};
use libp2p_identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p_mplex::MplexConfig;
use libp2p_noise::{Keypair, X25519Spec};
use libp2p_rendezvous::behaviour::{Difficulty, Event as RendezvousEvent, Rendezvous};
use libp2p_swarm::Swarm;
use libp2p_swarm::SwarmEvent;
use libp2p_tcp::TcpConfig;
use libp2p_yamux::YamuxConfig;
use std::str::FromStr;
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

fn main() {
    let identity = identity::Keypair::generate_ed25519();
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
    let rendezvous = Rendezvous::new(identity, 10000, Difficulty::from_u32(2).unwrap());

    let mut swarm = Swarm::new(
        transport,
        MyBehaviour {
            identify,
            rendezvous,
        },
        peer_id,
    );

    let _ = swarm.listen_on("/ip4/127.0.0.1/tcp/62343".parse().unwrap());

    swarm
        .dial_addr("/ip4/127.0.0.1/tcp/62649".parse().unwrap())
        .unwrap();

    let server_peer_id =
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();

    task::block_on(async move {
        loop {
            let event = swarm.next().await;
            match event {
                Some(SwarmEvent::Behaviour(MyEvent::Identify(IdentifyEvent::Received {
                    ..
                }))) => {
                    swarm
                        .behaviour_mut()
                        .rendezvous
                        .register("rendezvous".to_string(), server_peer_id, None)
                        .unwrap();
                }
                Some(SwarmEvent::Behaviour(MyEvent::Rendezvous(event))) => {
                    println!("registered event: {:?}", event);
                }
                _ => {}
            };
        }
    })
}
