mod client;

use crate::client::WtClient;
use futures::channel::mpsc;
use futures::stream::StreamExt;
use futures::{future, AsyncReadExt, AsyncWriteExt, SinkExt};
use libp2p_core::multiaddr::Protocol;
use libp2p_core::muxing::{StreamMuxerBox, StreamMuxerExt};
use libp2p_core::transport::{Boxed, ListenerId, TransportEvent};
use libp2p_core::{Multiaddr, Transport};
use libp2p_identity::{Keypair, PeerId};
use libp2p_webtransport as webtransport;
use libp2p_webtransport::{CertHash, Certificate};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;
use time::ext::NumericalDuration;
use time::OffsetDateTime;
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn smoke() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (keypair, cert, certhashes) = generate_keypair_and_certificate();
    let (mut listener_tx, mut listener_rx) = mpsc::channel(1);

    tokio::spawn(async move {
        let (peer_id, mut transport) = create_transport(keypair, cert);
        let addr =
            start_listening(&mut transport, "/ip4/127.0.0.1/udp/0/quic-v1/webtransport").await;

        listener_tx.send((peer_id, addr)).await.unwrap();

        loop {
            if let TransportEvent::Incoming { upgrade, .. } = transport.select_next_some().await {
                let (_, mut connection) = upgrade.await.unwrap();

                loop {
                    let Ok(mut inbound_stream) = future::poll_fn(|cx| {
                        let _ = connection.poll_unpin(cx)?;
                        connection.poll_inbound_unpin(cx)
                    })
                    .await
                    else {
                        return;
                    };

                    let mut pong = [0u8; 4];
                    inbound_stream.write_all(b"PING").await.unwrap();
                    inbound_stream.flush().await.unwrap();
                    inbound_stream.read_exact(&mut pong).await.unwrap();
                    assert_eq!(&pong, b"PONG");
                }
            }
        }
    });

    let (mut complete_tx, mut complete_rx) = mpsc::channel(1);

    tokio::spawn(async move {
        if let Some((peer_id, addr)) = listener_rx.next().await {
            let socket_addr = multiaddr_to_socketaddr(&addr).unwrap();
            let url = format!(
                "https://{}/.well-known/libp2p-webtransport?type=noise",
                socket_addr
            );
            let mut client = WtClient::new(url, peer_id, certhashes);
            let mut stream = client.connect().await.unwrap();

            // assert_eq!(peer_id, actual_peer_id);

            let mut ping = [0u8; 4];
            stream.write_all(b"PONG").await.unwrap();
            stream.flush().await.unwrap();
            stream.read_exact(&mut ping).await.unwrap();
            assert_eq!(&ping, b"PING");

            complete_tx.send(()).await.unwrap();
        }
    });

    tokio::time::timeout(Duration::from_secs(30), async move {
        complete_rx.next().await.unwrap();
    })
    .await
    .unwrap();
}

fn create_transport(
    keypair: Keypair,
    certificate: Certificate,
) -> (PeerId, Boxed<(PeerId, StreamMuxerBox)>) {
    let peer_id = keypair.public().to_peer_id();
    let config = webtransport::Config::new(&keypair, certificate);
    let transport = webtransport::GenTransport::new(config)
        .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        .boxed();

    (peer_id, transport)
}

fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Udp(port)) => Some(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Udp(port)) => Some(SocketAddr::new(ip.into(), port)),
        _ => None,
    }
}

async fn start_listening(transport: &mut Boxed<(PeerId, StreamMuxerBox)>, addr: &str) -> Multiaddr {
    transport
        .listen_on(ListenerId::next(), addr.parse().unwrap())
        .unwrap();
    match transport.next().await {
        Some(TransportEvent::NewAddress { listen_addr, .. }) => listen_addr,
        e => panic!("{e:?}"),
    }
}

fn generate_keypair_and_certificate() -> (Keypair, webtransport::Certificate, HashSet<CertHash>) {
    let keypair = Keypair::generate_ed25519();
    let not_before = OffsetDateTime::now_utc().checked_sub(1.days()).unwrap();
    let cert = Certificate::generate(&keypair, not_before).expect("Generate certificate");
    let mut certhashes: HashSet<CertHash> = HashSet::with_capacity(1);
    let hash = cert.cert_hash();
    certhashes.insert(hash);

    (keypair, cert, certhashes)
}
