use async_std::task;
use futures::StreamExt;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::{SelectUpgrade, Version};
use libp2p_core::PeerId;
use libp2p_core::{identity, Transport};
use libp2p_mplex::MplexConfig;
use libp2p_noise::{Keypair, X25519Spec};
use libp2p_rendezvous::behaviour::{Difficulty, Rendezvous};
use libp2p_swarm::Swarm;
use libp2p_tcp::TcpConfig;
use libp2p_yamux::YamuxConfig;
use std::time::Duration;

fn main() {
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

    let difficulty = Difficulty::from_u32(2).unwrap();
    let behaviour = Rendezvous::new(identity, 1000, difficulty);

    let mut swarm = Swarm::new(transport, behaviour, peer_id);

    println!("peer id: {}", swarm.local_peer_id());

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/62649".parse().unwrap())
        .unwrap();

    task::block_on(async move {
        loop {
            let event = swarm.next().await;
            println!("swarm event: {:?}", event);
        }
    });
}
