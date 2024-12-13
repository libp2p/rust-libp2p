use std::time::Duration;

use futures::{channel::oneshot, StreamExt};
use libp2p_core::{
    transport::{DialOpts, ListenerId, PortUse},
    Endpoint, Transport,
};
use libp2p_quic as quic;

#[async_std::test]
async fn close_implies_flush() {
    let (alice, bob) = connected_peers().await;

    libp2p_muxer_test_harness::close_implies_flush(alice, bob).await;
}

#[async_std::test]
async fn read_after_close() {
    let (alice, bob) = connected_peers().await;

    libp2p_muxer_test_harness::read_after_close(alice, bob).await;
}

async fn connected_peers() -> (quic::Connection, quic::Connection) {
    let mut dialer = new_transport().boxed();
    let mut listener = new_transport().boxed();

    listener
        .listen_on(
            ListenerId::next(),
            "/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap(),
        )
        .unwrap();
    let listen_address = listener.next().await.unwrap().into_new_address().unwrap();

    let (dialer_conn_sender, dialer_conn_receiver) = oneshot::channel();
    let (listener_conn_sender, listener_conn_receiver) = oneshot::channel();

    async_std::task::spawn(async move {
        let (upgrade, _) = listener.next().await.unwrap().into_incoming().unwrap();

        async_std::task::spawn(async move {
            let (_, connection) = upgrade.await.unwrap();

            let _ = listener_conn_sender.send(connection);
        });

        loop {
            listener.next().await;
        }
    });
    let dial_fut = dialer
        .dial(
            listen_address,
            DialOpts {
                role: Endpoint::Dialer,
                port_use: PortUse::Reuse,
            },
        )
        .unwrap();
    async_std::task::spawn(async move {
        let connection = dial_fut.await.unwrap().1;

        let _ = dialer_conn_sender.send(connection);
    });

    async_std::task::spawn(async move {
        loop {
            dialer.next().await;
        }
    });

    futures::future::try_join(dialer_conn_receiver, listener_conn_receiver)
        .await
        .unwrap()
}

fn new_transport() -> quic::async_std::Transport {
    let keypair = libp2p_identity::Keypair::generate_ed25519();
    let mut config = quic::Config::new(&keypair);
    config.handshake_timeout = Duration::from_secs(1);

    quic::async_std::Transport::new(config)
}
