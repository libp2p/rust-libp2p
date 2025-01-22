use futures::channel::mpsc;
use futures::stream::StreamExt;
use futures::{future, AsyncReadExt, AsyncWriteExt, SinkExt};
use libp2p_core::multiaddr::Protocol;
use libp2p_core::muxing::{StreamMuxerBox, StreamMuxerExt};
use libp2p_core::transport::{Boxed, ListenerId, TransportEvent};
use libp2p_core::upgrade::OutboundConnectionUpgrade;
use libp2p_core::{Multiaddr, Transport, UpgradeInfo};
use libp2p_identity::{Keypair, PeerId};
use libp2p_webtransport as webtransport;
use libp2p_webtransport::{CertHash, Certificate, Stream};
use std::collections::HashSet;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use time::ext::NumericalDuration;
use time::OffsetDateTime;
use tracing_subscriber::EnvFilter;
use wtransport::error::{ConnectingError, ConnectionError, StreamOpeningError};
use wtransport::{ClientConfig, Connection, Endpoint};

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

#[derive(Debug, thiserror::Error)]
enum ClientError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Connecting(#[from] ConnectingError),

    #[error(transparent)]
    Connection(#[from] ConnectionError),

    #[error(transparent)]
    Stream(#[from] StreamOpeningError),
}

struct WtClient {
    url: String,
    remote_peer_id: PeerId,
    keypair: Keypair,
    certhashes: HashSet<CertHash>,
    connection: Option<Arc<Connection>>,
}

impl WtClient {
    fn new(url: String, remote_peer_id: PeerId, certhashes: HashSet<CertHash>) -> Self {
        let keypair = Keypair::generate_ed25519();
        WtClient {
            url,
            remote_peer_id,
            keypair,
            certhashes,
            connection: None,
        }
    }

    async fn connect(&mut self) -> Result<Stream, ClientError> {
        let client_tls = libp2p_tls::make_webtransport_client_config(
            Some(self.remote_peer_id),
            alpn_protocols(),
        );

        let config = ClientConfig::builder()
            .with_bind_default()
            .with_custom_tls(client_tls)
            .build();

        tracing::debug!("Connecting to {}", self.url.as_str());

        let con = Endpoint::client(config)?.connect(self.url.as_str()).await?;
        self.connection = Some(Arc::new(con));

        tracing::debug!("Connection is established to {}", self.url.as_str());

        self.authenticate().await?;
        let stream = self.create_bidirectional_stream().await?;

        Ok(stream)
    }

    async fn create_bidirectional_stream(&mut self) -> Result<Stream, ClientError> {
        match &self.connection {
            None => {
                panic!("There is no open connection!")
            }
            Some(arc_con) => {
                let con = Arc::clone(arc_con);
                let opening_bi_stream = con.open_bi().await?;

                tracing::debug!("Opening bi stream is created");

                let (send, recv) = opening_bi_stream.await?;
                let stream = webtransport::Stream::new(send, recv);

                tracing::debug!("Stream is created");

                Ok(stream)
            }
        }
    }

    async fn authenticate(&mut self) -> Result<(), ClientError> {
        let stream = self.create_bidirectional_stream().await?;
        let mut noise = libp2p_noise::Config::new(&self.keypair).unwrap();

        if !self.certhashes.is_empty() {
            noise = noise.with_webtransport_certhashes(self.certhashes.clone());
        }

        tracing::debug!("Noise upgrade_outbound");
        let info = noise.protocol_info().next().unwrap_or_default();
        let _ = noise.upgrade_outbound(stream, info).await.unwrap();

        Ok(())
    }
}

fn alpn_protocols() -> Vec<Vec<u8>> {
    vec![b"libp2p".to_vec(), b"h3".to_vec()]
}
