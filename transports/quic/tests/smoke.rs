#![cfg(any(feature = "async-std", feature = "tokio"))]

use futures::channel::mpsc;
use futures::future::Either;
use futures::stream::StreamExt;
use futures::{future, AsyncReadExt, AsyncWriteExt, SinkExt};
use libp2p::core::multiaddr::Protocol;
use libp2p::core::Transport;
use libp2p::{noise, tcp, yamux, Multiaddr};
use libp2p_core::either::EitherOutput;
use libp2p_core::muxing::{StreamMuxerBox, StreamMuxerExt};
use libp2p_core::transport::{Boxed, OrTransport, TransportEvent};
use libp2p_core::{upgrade, PeerId};
use libp2p_quic as quic;
use quic::Provider;
use rand::RngCore;
use std::future::Future;
use std::io;
use std::num::NonZeroU8;
use std::time::Duration;

#[cfg(feature = "tokio")]
#[tokio::test]
async fn tokio_smoke() {
    smoke::<quic::tokio::Provider>().await
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn async_std_smoke() {
    smoke::<quic::async_std::Provider>().await
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn dial_failure() {
    let _ = env_logger::try_init();
    let mut a = create_transport::<quic::async_std::Provider>().1;
    let mut b = create_transport::<quic::async_std::Provider>().1;

    let addr = start_listening(&mut a, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    drop(a); // stop a so b can never reach it

    match dial(&mut b, addr).await {
        Ok(_) => panic!("Expected dial to fail"),
        Err(error) => {
            assert_eq!("Handshake with the remote timed out.", error.to_string())
        }
    };
}

#[cfg(feature = "tokio")]
#[tokio::test]
async fn endpoint_reuse() {
    let _ = env_logger::try_init();
    let (_, mut a_transport) = create_transport::<quic::tokio::Provider>();
    let (_, mut b_transport) = create_transport::<quic::tokio::Provider>();

    let a_addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let ((_, b_send_back_addr, _), _) =
        connect(&mut a_transport, &mut b_transport, a_addr.clone()).await;

    // Expect the dial to fail since b is not listening on an address.
    match dial(&mut a_transport, b_send_back_addr).await {
        Ok(_) => panic!("Expected dial to fail"),
        Err(error) => {
            assert_eq!("Handshake with the remote timed out.", error.to_string())
        }
    };

    let b_addr = start_listening(&mut b_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let ((_, a_send_back_addr, _), _) = connect(&mut b_transport, &mut a_transport, b_addr).await;

    assert_eq!(a_send_back_addr, a_addr);
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn ipv4_dial_ipv6() {
    let _ = env_logger::try_init();
    let (a_peer_id, mut a_transport) = create_transport::<quic::async_std::Provider>();
    let (b_peer_id, mut b_transport) = create_transport::<quic::async_std::Provider>();

    let a_addr = start_listening(&mut a_transport, "/ip6/::1/udp/0/quic-v1").await;
    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, a_addr).await;

    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

#[cfg(feature = "async-std")]
#[async_std::test]
#[ignore] // Transport currently does not validate PeerId. Enable once we make use of PeerId validation in rustls.
async fn wrong_peerid() {
    use libp2p::PeerId;

    let (a_peer_id, mut a_transport) = create_transport::<quic::async_std::Provider>();
    let (b_peer_id, mut b_transport) = create_transport::<quic::async_std::Provider>();

    let a_addr = start_listening(&mut a_transport, "/ip6/::1/udp/0/quic-v1").await;
    let a_addr_random_peer = a_addr.with(Protocol::P2p(PeerId::random().into()));

    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, a_addr_random_peer).await;

    assert_ne!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

#[cfg(feature = "async-std")]
fn new_tcp_quic_transport() -> (PeerId, Boxed<(PeerId, StreamMuxerBox)>) {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let mut config = quic::Config::new(&keypair);
    config.handshake_timeout = Duration::from_secs(1);

    let quic_transport = quic::async_std::Transport::new(config);
    let tcp_transport = tcp::async_io::Transport::new(tcp::Config::default())
        .upgrade(upgrade::Version::V1)
        .authenticate(
            noise::NoiseConfig::xx(
                noise::Keypair::<noise::X25519Spec>::new()
                    .into_authentic(&keypair)
                    .unwrap(),
            )
            .into_authenticated(),
        )
        .multiplex(yamux::YamuxConfig::default());

    let transport = OrTransport::new(quic_transport, tcp_transport)
        .map(|either_output, _| match either_output {
            EitherOutput::First((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            EitherOutput::Second((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed();

    (peer_id, transport)
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn tcp_and_quic() {
    let (a_peer_id, mut a_transport) = new_tcp_quic_transport();
    let (b_peer_id, mut b_transport) = new_tcp_quic_transport();

    let quic_addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let tcp_addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/tcp/0").await;

    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, quic_addr).await;
    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);

    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, tcp_addr).await;
    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

// Note: This test should likely be ported to the muxer compliance test suite.
#[cfg(feature = "async-std")]
#[test]
fn concurrent_connections_and_streams_async_std() {
    let _ = env_logger::try_init();

    quickcheck::QuickCheck::new()
        .min_tests_passed(1)
        .quickcheck(prop::<quic::async_std::Provider> as fn(_, _) -> _);
}

// Note: This test should likely be ported to the muxer compliance test suite.
#[cfg(feature = "tokio")]
#[test]
fn concurrent_connections_and_streams_tokio() {
    let _ = env_logger::try_init();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    quickcheck::QuickCheck::new()
        .min_tests_passed(1)
        .quickcheck(prop::<quic::tokio::Provider> as fn(_, _) -> _);
}

async fn smoke<P: Provider>() {
    let _ = env_logger::try_init();

    let (a_peer_id, mut a_transport) = create_transport::<P>();
    let (b_peer_id, mut b_transport) = create_transport::<P>();

    let addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, addr).await;

    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

fn generate_tls_keypair() -> libp2p::identity::Keypair {
    libp2p::identity::Keypair::generate_ed25519()
}

fn create_transport<P: Provider>() -> (PeerId, Boxed<(PeerId, StreamMuxerBox)>) {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();

    let transport = quic::GenTransport::<P>::new(quic::Config::new(&keypair))
        .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        .boxed();

    (peer_id, transport)
}

async fn start_listening(transport: &mut Boxed<(PeerId, StreamMuxerBox)>, addr: &str) -> Multiaddr {
    transport.listen_on(addr.parse().unwrap()).unwrap();
    match transport.next().await {
        Some(TransportEvent::NewAddress { listen_addr, .. }) => listen_addr,
        e => panic!("{:?}", e),
    }
}

fn prop<P: Provider + BlockOn>(
    number_listeners: NonZeroU8,
    number_streams: NonZeroU8,
) -> quickcheck::TestResult {
    const BUFFER_SIZE: usize = 4096 * 10;

    let number_listeners = u8::from(number_listeners) as usize;
    let number_streams = u8::from(number_streams) as usize;

    if number_listeners > 10 || number_streams > 10 {
        return quickcheck::TestResult::discard();
    }

    let (listeners_tx, mut listeners_rx) = mpsc::channel(number_listeners);

    log::info!("Creating {number_streams} streams on {number_listeners} connections");

    // Spawn the listener nodes.
    for _ in 0..number_listeners {
        P::spawn({
            let mut listeners_tx = listeners_tx.clone();

            async move {
                let (peer_id, mut listener) = create_transport::<P>();
                let addr = start_listening(&mut listener, "/ip4/127.0.0.1/udp/0/quic-v1").await;

                listeners_tx.send((peer_id, addr)).await.unwrap();

                loop {
                    if let TransportEvent::Incoming { upgrade, .. } =
                        listener.select_next_some().await
                    {
                        let (_, connection) = upgrade.await.unwrap();

                        P::spawn(answer_inbound_streams::<P, BUFFER_SIZE>(connection));
                    }
                }
            }
        })
    }

    let (completed_streams_tx, completed_streams_rx) =
        mpsc::channel(number_streams * number_listeners);

    // For each listener node start `number_streams` requests.
    P::spawn(async move {
        let (_, mut dialer) = create_transport::<P>();

        while let Some((_, listener_addr)) = listeners_rx.next().await {
            let (_, connection) = dial(&mut dialer, listener_addr.clone()).await.unwrap();

            P::spawn(open_outbound_streams::<P, BUFFER_SIZE>(
                connection,
                number_streams,
                completed_streams_tx.clone(),
            ))
        }

        // Drive the dialer.
        loop {
            dialer.next().await;
        }
    });

    let completed_streams = number_streams * number_listeners;

    // Wait for all streams to complete.
    P::block_on(
        completed_streams_rx
            .take(completed_streams as usize)
            .collect::<Vec<_>>(),
        Duration::from_secs(30),
    );

    quickcheck::TestResult::passed()
}

async fn answer_inbound_streams<P: Provider, const BUFFER_SIZE: usize>(
    mut connection: StreamMuxerBox,
) {
    loop {
        let mut inbound_stream = match future::poll_fn(|cx| {
            let _ = connection.poll_unpin(cx)?;

            connection.poll_inbound_unpin(cx)
        })
        .await
        {
            Ok(s) => s,
            Err(_) => return,
        };

        P::spawn(async move {
            // FIXME: Need to write _some_ data before we can read on both sides.
            // Do a ping-pong exchange.
            {
                let mut pong = [0u8; 4];
                inbound_stream.write_all(b"PING").await.unwrap();
                inbound_stream.flush().await.unwrap();
                inbound_stream.read_exact(&mut pong).await.unwrap();
                assert_eq!(&pong, b"PONG");
            }

            let mut data = vec![0; BUFFER_SIZE];

            inbound_stream.read_exact(&mut data).await.unwrap();
            inbound_stream.write_all(&data).await.unwrap();
            inbound_stream.close().await.unwrap();
        });
    }
}

async fn open_outbound_streams<P: Provider, const BUFFER_SIZE: usize>(
    mut connection: StreamMuxerBox,
    number_streams: usize,
    completed_streams_tx: mpsc::Sender<()>,
) {
    for _ in 0..number_streams {
        let mut outbound_stream = future::poll_fn(|cx| {
            let _ = connection.poll_unpin(cx)?;

            connection.poll_outbound_unpin(cx)
        })
        .await
        .unwrap();

        P::spawn({
            let mut completed_streams_tx = completed_streams_tx.clone();

            async move {
                // FIXME: Need to write _some_ data before we can read on both sides.
                // Do a ping-pong exchange.
                {
                    let mut ping = [0u8; 4];
                    outbound_stream.write_all(b"PONG").await.unwrap();
                    outbound_stream.flush().await.unwrap();
                    outbound_stream.read_exact(&mut ping).await.unwrap();
                    assert_eq!(&ping, b"PING");
                }

                let mut data = vec![0; BUFFER_SIZE];
                rand::thread_rng().fill_bytes(&mut data);

                let mut received = Vec::new();

                outbound_stream.write_all(&data).await.unwrap();
                outbound_stream.flush().await.unwrap();
                outbound_stream.read_to_end(&mut received).await.unwrap();

                assert_eq!(received, data);

                completed_streams_tx.send(()).await.unwrap();
            }
        });
    }

    log::info!("Created {number_streams} streams");

    while future::poll_fn(|cx| connection.poll_unpin(cx))
        .await
        .is_ok()
    {}
}

/// Helper function for driving two transports until they established a connection.
async fn connect(
    listener: &mut Boxed<(PeerId, StreamMuxerBox)>,
    dialer: &mut Boxed<(PeerId, StreamMuxerBox)>,
    addr: Multiaddr,
) -> (
    (PeerId, Multiaddr, StreamMuxerBox),
    (PeerId, StreamMuxerBox),
) {
    future::join(
        async {
            let (upgrade, send_back_addr) =
                listener.select_next_some().await.into_incoming().unwrap();
            let (peer_id, connection) = upgrade.await.unwrap();

            (peer_id, send_back_addr, connection)
        },
        async { dial(dialer, addr).await.unwrap() },
    )
    .await
}

/// Helper function for dialling that also polls the `Transport`.
async fn dial(
    transport: &mut Boxed<(PeerId, StreamMuxerBox)>,
    addr: Multiaddr,
) -> io::Result<(PeerId, StreamMuxerBox)> {
    match future::select(transport.dial(addr).unwrap(), transport.next()).await {
        Either::Left((conn, _)) => conn,
        Either::Right((event, _)) => {
            panic!("Unexpected event: {event:?}")
        }
    }
}

trait BlockOn {
    fn block_on<R>(future: impl Future<Output = R> + Send, timeout: Duration) -> R;
}

#[cfg(feature = "async-std")]
impl BlockOn for libp2p_quic::async_std::Provider {
    fn block_on<R>(future: impl Future<Output = R> + Send, timeout: Duration) -> R {
        async_std::task::block_on(async_std::future::timeout(timeout, future)).unwrap()
    }
}

#[cfg(feature = "tokio")]
impl BlockOn for libp2p_quic::tokio::Provider {
    fn block_on<R>(future: impl Future<Output = R> + Send, timeout: Duration) -> R {
        tokio::runtime::Handle::current()
            .block_on(tokio::time::timeout(timeout, future))
            .unwrap()
    }
}
