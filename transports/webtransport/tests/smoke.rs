use std::net::SocketAddr;

use futures::join;
use futures::stream::StreamExt;
use time::ext::NumericalDuration;
use time::OffsetDateTime;
use wtransport::ClientConfig;
use wtransport::Endpoint;

use libp2p_core::multiaddr::Protocol;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::{Boxed, ListenerId, TransportEvent};
use libp2p_core::{Multiaddr, Transport};
use libp2p_identity::{Keypair, PeerId};
use libp2p_webtransport as webtransport;

#[tokio::test]
async fn smoke() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let (keypair, cert) = generate_keypair_and_certificate();
    let config = webtransport::Config::new(&keypair, cert);
    let mut transport = webtransport::GenTransport::new(config)
        .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        .boxed();
    let addr = start_listening(&mut transport, "/ip4/127.0.0.1/udp/0/quic-v1/webtransport").await;
    let socket_addr = multiaddr_to_socketaddr(&addr).unwrap();

    let a = async move {
        loop {
            match &mut transport.next().await {
                Some(TransportEvent::Incoming {
                    listener_id,
                    upgrade,
                    ..
                }) => {
                    tracing::debug!("Got incoming event. listener_id={}", listener_id);
                    match upgrade.await {
                        Ok((peer_id, _mutex)) => {
                            tracing::debug!("Connection is opened. peer_id={}", peer_id);
                            return Some(peer_id);
                        }
                        Err(e) => {
                            tracing::error!("Upgrade got an error {:?}", e);
                            return None;
                        }
                    }
                }
                Some(e) => tracing::debug!("Got event {:?}", e),
                e => {
                    tracing::error!("MY_TEST Got an error {:?}", e);
                    return None;
                }
            }
        }
    };

    let url = format!(
        "https://{}/.well-known/libp2p-webtransport?type=noise",
        socket_addr
    );
    let b = async move {
        let client_key_pair = Keypair::generate_ed25519();
        let client_tls = libp2p_tls::make_client_config(&client_key_pair, None).unwrap();
        let config = ClientConfig::builder()
            .with_bind_default()
            .with_custom_tls(client_tls)
            .build();

        match Endpoint::client(config)
            .unwrap()
            .connect(url.as_str())
            .await
        {
            Ok(_) => {}
            Err(_) => {}
        }
    };

    matches!(join!(a, b), (Some(_id), ()));
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

fn generate_keypair_and_certificate() -> (Keypair, webtransport::Certificate) {
    let keypair = Keypair::generate_ed25519();
    let not_before = OffsetDateTime::now_utc().checked_sub(1.days()).unwrap();
    let cert =
        webtransport::Certificate::generate(&keypair, not_before).expect("Generate certificate");

    (keypair, cert)
}
