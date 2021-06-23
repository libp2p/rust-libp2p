use async_std::task;
use futures::StreamExt;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::{SelectUpgrade, Version};
use libp2p_core::PeerId;
use libp2p_core::{identity, Transport};
use libp2p_mplex::MplexConfig;
use libp2p_noise::{Keypair, X25519Spec};
use libp2p_rendezvous::behaviour::{Event, Rendezvous};
use libp2p_swarm::Swarm;
use libp2p_swarm::SwarmEvent;
use libp2p_tcp::TcpConfig;
use libp2p_yamux::YamuxConfig;
use std::str::FromStr;
use std::time::Duration;
use Event::Discovered;

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

    let behaviour = Rendezvous::new(identity, 10000);

    let mut swarm = Swarm::new(transport, behaviour, peer_id);

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
                    server_peer_id,
                );
            };
            if let Some(SwarmEvent::Behaviour(Discovered { registrations, .. })) = event {
                println!("discovered: {:?}", registrations.values());
            };
        }
    })
}
