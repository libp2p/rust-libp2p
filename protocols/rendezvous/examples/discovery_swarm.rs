use async_std::task;
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::{SelectUpgrade, Version};
use libp2p::core::PeerId;
use libp2p::core::{identity, Transport};
use libp2p::mplex::MplexConfig;
use libp2p::noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p::rendezvous;
use libp2p::rendezvous::Rendezvous;
use libp2p::swarm::Swarm;
use libp2p::swarm::SwarmEvent;
use libp2p::tcp::TcpConfig;
use libp2p::yamux::YamuxConfig;
use std::str::FromStr;
use std::time::Duration;

fn main() {
    let identity = identity::Keypair::generate_ed25519();

    let transport = TcpConfig::new()
        .upgrade(Version::V1)
        .authenticate(
            NoiseConfig::xx(
                Keypair::<X25519Spec>::new()
                    .into_authentic(&identity)
                    .expect("failed to create dh_keys"),
            )
            .into_authenticated(),
        )
        .multiplex(SelectUpgrade::new(
            YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let local_peer_id = PeerId::from(identity.public());
    let mut swarm = Swarm::new(transport, Rendezvous::new(identity, 10000), local_peer_id);

    let _ = swarm.dial_addr("/ip4/127.0.0.1/tcp/62649".parse().unwrap());

    let server_peer_id =
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();

    task::block_on(async move {
        loop {
            let event = swarm.next().await;
            if let Some(SwarmEvent::ConnectionEstablished { .. }) = event {
                swarm.behaviour_mut().discover(
                    Some("rendezvous".to_string()),
                    None,
                    None,
                    server_peer_id,
                );
            };
            if let Some(SwarmEvent::Behaviour(rendezvous::Event::Discovered {
                registrations,
                ..
            })) = event
            {
                println!("discovered: {:?}", registrations.values());
            };
        }
    })
}
