use libp2p_core::{
    identity,
    muxing::StreamMuxerBox,
    transport::{self, Transport},
    upgrade, PeerId,
};
use libp2p_noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p_tcp::TcpConfig;
use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter};

pub fn mk_transport() -> (PeerId, transport::Boxed<(PeerId, StreamMuxerBox)>) {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().into();
    let noise_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    (
        peer_id,
        TcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(NoiseConfig::xx(noise_keys).into_authenticated())
            .multiplex(libp2p_yamux::YamuxConfig::default())
            .boxed(),
    )
}

pub fn setup_logger() {
    tracing_log::LogTracer::init().ok();
    let env = std::env::var(EnvFilter::DEFAULT_ENV).unwrap_or_else(|_| "info".to_owned());
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_span_events(FmtSpan::ACTIVE | FmtSpan::CLOSE)
        .with_env_filter(EnvFilter::new(env))
        .with_writer(std::io::stderr)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok();
}
