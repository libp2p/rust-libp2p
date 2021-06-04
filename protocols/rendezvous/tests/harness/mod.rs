use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::upgrade::Version;
use libp2p_core::{identity, transport, Multiaddr, PeerId, Transport};
use libp2p_noise as noise;
use libp2p_tcp::TcpConfig;
use libp2p_yamux::YamuxConfig;

pub fn mk_transport(id_keys: identity::Keypair) -> transport::Boxed<(PeerId, StreamMuxerBox)> {
    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();

    TcpConfig::new()
        .nodelay(true)
        .upgrade(Version::V1)
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(YamuxConfig::default())
        .boxed()
}

pub fn get_rand_listen_addr() -> Multiaddr {
    let address_port = rand::random::<u16>();
    let addr = format!("/ip4/127.0.0.1/tcp/{}", address_port)
        .parse::<Multiaddr>()
        .unwrap();

    addr
}
