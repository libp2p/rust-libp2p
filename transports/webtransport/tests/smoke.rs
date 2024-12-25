use libp2p_identity::{Keypair, PeerId};
use tracing_subscriber::EnvFilter;

use futures::{
    channel::{mpsc, oneshot},
    future,
    future::{poll_fn, BoxFuture, Either},
    stream::StreamExt,
    AsyncReadExt, AsyncWriteExt, FutureExt, SinkExt,
};

use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::Boxed;
use libp2p_webtransport as webtransport;
use time::ext::NumericalDuration;
use time::OffsetDateTime;

#[tokio::test]
async fn smoke() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

fn create_transport() -> (PeerId, Boxed<(PeerId, StreamMuxerBox)>) {
    let (keypair, cert) = generate_keypair_and_certificate();
    // let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let config = webtransport::Config::new(&keypair, cert);
    // with_config(&mut config);
    let transport = webtransport::GenTransport::new(config)
        .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        .boxed();

    (peer_id, transport)
}

fn generate_keypair_and_certificate() -> (Keypair, webtransport::Certificate) {
    let keypair = Keypair::generate_ed25519();
    let not_before = OffsetDateTime::now_utc().checked_sub(1.days()).unwrap();
    let cert =
        webtransport::Certificate::generate(&keypair, not_before).expect("Generate certificate");

    (keypair, cert)
}
