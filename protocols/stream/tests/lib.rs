use std::time::Duration;

use futures::{AsyncReadExt as _, AsyncWriteExt as _, StreamExt as _};
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

    let mut swarm1 = Swarm::new_ephemeral(|_| stream::Behaviour::default());
    let mut swarm2 = Swarm::new_ephemeral(|_| stream::Behaviour::default());

    let mut control = swarm1.behaviour_mut().new_control(PROTOCOL);
    let mut incoming = swarm2.behaviour_mut().accept(PROTOCOL).unwrap();

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

    let mut peer_control = control.peer(swarm2_peer_id).await.unwrap();
    let mut stream = peer_control.open_stream().await.unwrap();

    let mut buf = [0u8; 1];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!([42], buf);

    handle.abort();

    let error = peer_control.open_stream().await.unwrap_err();
    assert!(matches!(error, OpenStreamError::UnsupportedProtocol));
}

#[tokio::test]
async fn peer_control_keep_alive() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env()
                .unwrap(),
        )
        .with_test_writer()
        .try_init();

    let mut swarm1 = Swarm::new_ephemeral(|_| stream::Behaviour::default());
    let mut swarm2 = Swarm::new_ephemeral(|_| stream::Behaviour::default());

    let mut control1 = swarm1.behaviour_mut().new_control(PROTOCOL);
    let mut control2 = swarm2.behaviour_mut().new_control(PROTOCOL);
    let mut incoming = swarm2.behaviour_mut().accept(PROTOCOL).unwrap();

    swarm2.listen().with_memory_addr_external().await;
    swarm1.connect(&mut swarm2).await;

    let swarm1_peer_id = *swarm1.local_peer_id();
    let swarm2_peer_id = *swarm2.local_peer_id();

    let mut peer_control1 = control1.peer(swarm2_peer_id).await.unwrap();
    let peer_control2 = control2.peer(swarm1_peer_id).await.unwrap();

    let handle = tokio::spawn(async move {
        while let Some((_, mut stream)) = incoming.next().await {
            stream.write_all(&[42]).await.unwrap();
            stream.close().await.unwrap();
        }
    });
    tokio::spawn(swarm1.loop_on_next());
    tokio::spawn(swarm2.loop_on_next());

    // The idle timeout in `new_ephemeral` is 5 seconds.
    // Hence, if the keep-alive using `PeerControl` does not work, we would fail after this.
    tokio::time::sleep(Duration::from_secs(6)).await;

    let mut stream = peer_control1.open_stream().await.unwrap();

    let mut buf = [0u8; 1];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!([42], buf);

    drop(peer_control1);
    drop(peer_control2);

    // The idle timeout in `new_ephemeral` is 5 seconds.
    // Hence, if the keep-alive using `PeerControl` does not work, we would fail after this.
    tokio::time::sleep(Duration::from_secs(10)).await;

    let error = control1.peer(swarm2_peer_id).await.unwrap_err();
}
