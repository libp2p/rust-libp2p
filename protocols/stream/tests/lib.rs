use std::io;

use futures::{AsyncReadExt as _, AsyncWriteExt as _, StreamExt as _};
use libp2p_identity::PeerId;
use libp2p_stream as stream;
use libp2p_swarm::{StreamProtocol, Swarm};
use libp2p_swarm_test::SwarmExt as _;
use stream::OpenStreamError;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const PROTOCOL: StreamProtocol = StreamProtocol::new("/test");

#[tokio::test]
async fn dropping_incoming_streams_deregisters() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env()
                .unwrap(),
        )
        .with_test_writer()
        .try_init();

    let mut swarm1 = Swarm::new_ephemeral(|_| stream::Behaviour::new());
    let mut swarm2 = Swarm::new_ephemeral(|_| stream::Behaviour::new());

    let mut control = swarm1.behaviour().new_control();
    let mut incoming = swarm2.behaviour().new_control().accept(PROTOCOL).unwrap();

    swarm2.listen().with_memory_addr_external().await;
    swarm1.connect(&mut swarm2).await;

    let swarm2_peer_id = *swarm2.local_peer_id();

    let handle = tokio::spawn(async move {
        while let Some((_, mut stream)) = incoming.next().await {
            stream.write_all(&[42]).await.unwrap();
            stream.close().await.unwrap();
        }
    });
    tokio::spawn(swarm1.loop_on_next());
    tokio::spawn(swarm2.loop_on_next());

    let mut stream = control.open_stream(swarm2_peer_id, PROTOCOL).await.unwrap();

    let mut buf = [0u8; 1];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!([42], buf);

    handle.abort();
    let _ = handle.await;

    let error = control
        .open_stream(swarm2_peer_id, PROTOCOL)
        .await
        .unwrap_err();
    assert!(matches!(error, OpenStreamError::UnsupportedProtocol(_)));
}

#[tokio::test]
async fn dial_errors_are_propagated() {
    let swarm1 = Swarm::new_ephemeral(|_| stream::Behaviour::new());

    let mut control = swarm1.behaviour().new_control();
    tokio::spawn(swarm1.loop_on_next());

    let error = control
        .open_stream(PeerId::random(), PROTOCOL)
        .await
        .unwrap_err();

    let OpenStreamError::Io(e) = error else {
        panic!("Unexpected error: {error}")
    };

    assert_eq!(e.kind(), io::ErrorKind::NotConnected);
    assert_eq!("Dial error: no addresses for peer.", e.to_string());
}
