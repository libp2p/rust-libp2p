#![cfg(any(feature = "async-std", feature = "tokio"))]

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::future::{poll_fn, Either};
use futures::stream::StreamExt;
use futures::{future, AsyncReadExt, AsyncWriteExt, FutureExt, SinkExt};
use futures_timer::Delay;
use libp2p_core::muxing::{StreamMuxerBox, StreamMuxerExt, SubstreamBox};
use libp2p_core::transport::{Boxed, OrTransport, TransportEvent};
use libp2p_core::transport::{ListenerId, TransportError};
use libp2p_core::{multiaddr::Protocol, upgrade, Multiaddr, Transport};
use libp2p_identity::PeerId;
use libp2p_noise as noise;
use libp2p_quic as quic;
use libp2p_tcp as tcp;
use libp2p_yamux as yamux;
use quic::Provider;
use rand::RngCore;
use std::future::Future;
use std::io;
use std::num::NonZeroU8;
use std::task::Poll;
use std::time::Duration;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

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
    let mut a = create_default_transport::<quic::async_std::Provider>().1;
    let mut b = create_default_transport::<quic::async_std::Provider>().1;

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
    let (_, mut a_transport) = create_default_transport::<quic::tokio::Provider>();
    let (_, mut b_transport) = create_default_transport::<quic::tokio::Provider>();

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
    let (a_peer_id, mut a_transport) = create_default_transport::<quic::async_std::Provider>();
    let (b_peer_id, mut b_transport) = create_default_transport::<quic::async_std::Provider>();

    let a_addr = start_listening(&mut a_transport, "/ip6/::1/udp/0/quic-v1").await;
    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, a_addr).await;

    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

/// Tests that a [`Transport::dial`] wakes up the task previously polling [`Transport::poll`].
///
/// See https://github.com/libp2p/rust-libp2p/pull/3306 for context.
#[cfg(feature = "async-std")]
#[async_std::test]
async fn wrapped_with_delay() {
    let _ = env_logger::try_init();

    struct DialDelay(Arc<Mutex<Boxed<(PeerId, StreamMuxerBox)>>>);

    impl Transport for DialDelay {
        type Output = (PeerId, StreamMuxerBox);
        type Error = std::io::Error;
        type ListenerUpgrade = Pin<Box<dyn Future<Output = io::Result<Self::Output>> + Send>>;
        type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

        fn listen_on(
            &mut self,
            id: ListenerId,
            addr: Multiaddr,
        ) -> Result<(), TransportError<Self::Error>> {
            self.0.lock().unwrap().listen_on(id, addr)
        }

        fn remove_listener(&mut self, id: ListenerId) -> bool {
            self.0.lock().unwrap().remove_listener(id)
        }

        fn address_translation(
            &self,
            listen: &Multiaddr,
            observed: &Multiaddr,
        ) -> Option<Multiaddr> {
            self.0.lock().unwrap().address_translation(listen, observed)
        }

        /// Delayed dial, i.e. calling [`Transport::dial`] on the inner [`Transport`] not within the
        /// synchronous [`Transport::dial`] method, but within the [`Future`] returned by the outer
        /// [`Transport::dial`].
        fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
            let t = self.0.clone();
            Ok(async move {
                // Simulate DNS lookup. Giving the `Transport::poll` the chance to return
                // `Poll::Pending` and thus suspending its task, waiting for a wakeup from the dial
                // on the inner transport below.
                Delay::new(Duration::from_millis(100)).await;

                let dial = t.lock().unwrap().dial(addr).map_err(|e| match e {
                    TransportError::MultiaddrNotSupported(_) => {
                        panic!()
                    }
                    TransportError::Other(e) => e,
                })?;
                dial.await
            }
            .boxed())
        }

        fn dial_as_listener(
            &mut self,
            addr: Multiaddr,
        ) -> Result<Self::Dial, TransportError<Self::Error>> {
            self.0.lock().unwrap().dial_as_listener(addr)
        }

        fn poll(
            self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
            Pin::new(&mut *self.0.lock().unwrap()).poll(cx)
        }
    }

    let (a_peer_id, mut a_transport) = create_default_transport::<quic::async_std::Provider>();
    let (b_peer_id, mut b_transport) = {
        let (id, transport) = create_default_transport::<quic::async_std::Provider>();
        (id, DialDelay(Arc::new(Mutex::new(transport))).boxed())
    };

    // Spawn A
    let a_addr = start_listening(&mut a_transport, "/ip6/::1/udp/0/quic-v1").await;
    let listener = async_std::task::spawn(async move {
        let (upgrade, _) = a_transport
            .select_next_some()
            .await
            .into_incoming()
            .unwrap();
        let (peer_id, _) = upgrade.await.unwrap();

        peer_id
    });

    // Spawn B
    //
    // Note that the dial is spawned on a different task than the transport allowing the transport
    // task to poll the transport once and then suspend, waiting for the wakeup from the dial.
    let dial = async_std::task::spawn({
        let dial = b_transport.dial(a_addr).unwrap();
        async { dial.await.unwrap().0 }
    });
    async_std::task::spawn(async move { b_transport.next().await });

    let (a_connected, b_connected) = future::join(listener, dial).await;

    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

#[cfg(feature = "async-std")]
#[async_std::test]
#[ignore] // Transport currently does not validate PeerId. Enable once we make use of PeerId validation in rustls.
async fn wrong_peerid() {
    use libp2p_identity::PeerId;

    let (a_peer_id, mut a_transport) = create_default_transport::<quic::async_std::Provider>();
    let (b_peer_id, mut b_transport) = create_default_transport::<quic::async_std::Provider>();

    let a_addr = start_listening(&mut a_transport, "/ip6/::1/udp/0/quic-v1").await;
    let a_addr_random_peer = a_addr.with(Protocol::P2p(PeerId::random()));

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
        .authenticate(noise::Config::new(&keypair).unwrap())
        .multiplex(yamux::Config::default());

    let transport = OrTransport::new(quic_transport, tcp_transport)
        .map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
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

#[cfg(feature = "tokio")]
#[tokio::test]
async fn draft_29_support() {
    use std::task::Poll;

    use futures::{future::poll_fn, select};
    use libp2p_core::transport::TransportError;

    let _ = env_logger::try_init();

    let (_, mut a_transport) =
        create_transport::<quic::tokio::Provider>(|cfg| cfg.support_draft_29 = true);
    let (_, mut b_transport) =
        create_transport::<quic::tokio::Provider>(|cfg| cfg.support_draft_29 = true);

    // If a server supports draft-29 all its QUIC addresses can be dialed on draft-29 or version-1
    let a_quic_addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/quic").await;
    let a_quic_mapped_addr = swap_protocol!(a_quic_addr, Quic => QuicV1);
    let a_quic_v1_addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let a_quic_v1_mapped_addr = swap_protocol!(a_quic_v1_addr, QuicV1 => Quic);

    connect(&mut a_transport, &mut b_transport, a_quic_addr.clone()).await;
    connect(&mut a_transport, &mut b_transport, a_quic_mapped_addr).await;
    connect(&mut a_transport, &mut b_transport, a_quic_v1_addr).await;
    connect(&mut a_transport, &mut b_transport, a_quic_v1_mapped_addr).await;

    let (_, mut c_transport) =
        create_transport::<quic::tokio::Provider>(|cfg| cfg.support_draft_29 = false);
    assert!(matches!(
        c_transport.dial(a_quic_addr),
        Err(TransportError::MultiaddrNotSupported(_))
    ));

    // Test disabling draft-29 on a server.
    let (_, mut d_transport) =
        create_transport::<quic::tokio::Provider>(|cfg| cfg.support_draft_29 = false);
    assert!(matches!(
        d_transport.listen_on(
            ListenerId::next(),
            "/ip4/127.0.0.1/udp/0/quic".parse().unwrap()
        ),
        Err(TransportError::MultiaddrNotSupported(_))
    ));
    let d_quic_v1_addr = start_listening(&mut d_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let d_quic_addr_mapped = swap_protocol!(d_quic_v1_addr, QuicV1 => Quic);
    let dial = b_transport.dial(d_quic_addr_mapped).unwrap();
    let drive_transports = poll_fn::<(), _>(|cx| {
        let _ = b_transport.poll_next_unpin(cx);
        let _ = d_transport.poll_next_unpin(cx);
        Poll::Pending
    });
    select! {
        _ = drive_transports.fuse() => {}
        result = dial.fuse() => {
            #[allow(clippy::single_match)]
            match result {
                Ok(_) => panic!("Unexpected success dialing version-1-only server with draft-29."),
                // FIXME: We currently get a Handshake timeout if the server does not support our version.
                // Correct would be to get an quinn error "VersionMismatch".
                Err(_) => {}
                // Err(e) => assert!(format!("{:?}", e).contains("VersionMismatch"), "Got unexpected error {}", e),
            }
        }
    }
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn backpressure() {
    let _ = env_logger::try_init();
    let max_stream_data = quic::Config::new(&generate_tls_keypair()).max_stream_data;

    let (mut stream_a, mut stream_b) = build_streams::<quic::async_std::Provider>().await;

    let data = vec![0; max_stream_data as usize - 1];

    stream_a.write_all(&data).await.unwrap();

    let more_data = vec![0; 1];
    assert!(stream_a.write(&more_data).now_or_never().is_none());

    let mut buf = vec![1; max_stream_data as usize - 1];
    stream_b.read_exact(&mut buf).await.unwrap();

    let mut buf = [0];
    assert!(stream_b.read(&mut buf).now_or_never().is_none());

    assert!(stream_a.write(&more_data).now_or_never().is_some());
}

#[cfg(feature = "async-std")]
#[async_std::test]
async fn read_after_peer_dropped_stream() {
    let _ = env_logger::try_init();
    let (mut stream_a, mut stream_b) = build_streams::<quic::async_std::Provider>().await;

    let data = vec![0; 10];

    stream_b.close().now_or_never();
    stream_a.write_all(&data).await.unwrap();
    stream_a.close().await.unwrap();
    drop(stream_a);

    stream_b.close().await.unwrap();
    let mut buf = Vec::new();
    stream_b.read_to_end(&mut buf).await.unwrap();
    assert_eq!(data, buf)
}

#[cfg(feature = "async-std")]
#[async_std::test]
#[should_panic]
async fn write_after_peer_dropped_stream() {
    let _ = env_logger::try_init();
    let (stream_a, mut stream_b) = build_streams::<quic::async_std::Provider>().await;
    drop(stream_a);
    futures_timer::Delay::new(Duration::from_millis(1)).await;

    let data = vec![0; 10];
    stream_b.write_all(&data).await.expect("Write failed.");
    stream_b.close().await.expect("Close failed.");
}

/// - A listens on 0.0.0.0:0
/// - B listens on 127.0.0.1:0
/// - A dials B
/// - Source port of A at B is the A's listen port
#[cfg(feature = "tokio")]
#[tokio::test]
async fn test_local_listener_reuse() {
    let (_, mut a_transport) = create_default_transport::<quic::tokio::Provider>();
    let (_, mut b_transport) = create_default_transport::<quic::tokio::Provider>();

    a_transport
        .listen_on(
            ListenerId::next(),
            "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
        )
        .unwrap();

    // wait until a listener reports a loopback address
    let a_listen_addr = 'outer: loop {
        let ev = a_transport.next().await.unwrap();
        let listen_addr = ev.into_new_address().unwrap();
        for proto in listen_addr.iter() {
            if let Protocol::Ip4(ip4) = proto {
                if ip4.is_loopback() {
                    break 'outer listen_addr;
                }
            }
        }
    };
    // If we do not poll until the end, `NewAddress` events may be `Ready` and `connect` function
    // below will panic due to an unexpected event.
    poll_fn(|cx| {
        let mut pinned = Pin::new(&mut a_transport);
        while pinned.as_mut().poll(cx).is_ready() {}
        Poll::Ready(())
    })
    .await;

    let b_addr = start_listening(&mut b_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let (_, send_back_addr, _) = connect(&mut b_transport, &mut a_transport, b_addr).await.0;
    assert_eq!(send_back_addr, a_listen_addr);
}

async fn smoke<P: Provider>() {
    let _ = env_logger::try_init();

    let (a_peer_id, mut a_transport) = create_default_transport::<P>();
    let (b_peer_id, mut b_transport) = create_default_transport::<P>();

    let addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, addr).await;

    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

async fn build_streams<P: Provider + Spawn>() -> (SubstreamBox, SubstreamBox) {
    let (_, mut a_transport) = create_default_transport::<P>();
    let (_, mut b_transport) = create_default_transport::<P>();

    let addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/quic-v1").await;
    let ((_, _, mut conn_a), (_, mut conn_b)) =
        connect(&mut a_transport, &mut b_transport, addr).await;
    let (stream_a_tx, stream_a_rx) = oneshot::channel();
    let (stream_b_tx, stream_b_rx) = oneshot::channel();

    P::spawn(async move {
        let mut stream_a_tx = Some(stream_a_tx);
        let mut stream_b_tx = Some(stream_b_tx);
        poll_fn::<(), _>(move |cx| {
            let _ = a_transport.poll_next_unpin(cx);
            let _ = conn_a.poll_unpin(cx);
            let _ = b_transport.poll_next_unpin(cx);
            let _ = conn_b.poll_unpin(cx);
            if stream_a_tx.is_some() {
                if let Poll::Ready(stream) = conn_a.poll_outbound_unpin(cx) {
                    let tx = stream_a_tx.take().unwrap();
                    tx.send(stream.unwrap()).unwrap();
                }
            }
            if stream_b_tx.is_some() {
                if let Poll::Ready(stream) = conn_b.poll_inbound_unpin(cx) {
                    let tx = stream_b_tx.take().unwrap();
                    tx.send(stream.unwrap()).unwrap();
                }
            }
            Poll::Pending
        })
        .await
    });
    let mut stream_a = stream_a_rx.map(Result::unwrap).await;

    // Send dummy byte to notify the peer of the new stream.
    let send_buf = [0];
    stream_a.write_all(&send_buf).await.unwrap();

    let mut stream_b = stream_b_rx.map(Result::unwrap).await;
    let mut recv_buf = [1];

    assert_eq!(stream_b.read(&mut recv_buf).await.unwrap(), 1);
    assert_eq!(send_buf, recv_buf);

    (stream_a, stream_b)
}

#[macro_export]
macro_rules! swap_protocol {
    ($addr:expr, $From:ident => $To:ident) => {
        $addr
            .into_iter()
            .map(|p| match p {
                Protocol::$From => Protocol::$To,
                _ => p,
            })
            .collect::<Multiaddr>()
    };
}

fn generate_tls_keypair() -> libp2p_identity::Keypair {
    libp2p_identity::Keypair::generate_ed25519()
}

fn create_default_transport<P: Provider>() -> (PeerId, Boxed<(PeerId, StreamMuxerBox)>) {
    create_transport::<P>(|_| {})
}

fn create_transport<P: Provider>(
    with_config: impl Fn(&mut quic::Config),
) -> (PeerId, Boxed<(PeerId, StreamMuxerBox)>) {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();
    let mut config = quic::Config::new(&keypair);
    with_config(&mut config);
    let transport = quic::GenTransport::<P>::new(config)
        .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        .boxed();

    (peer_id, transport)
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

fn prop<P: Provider + BlockOn + Spawn>(
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
                let (peer_id, mut listener) = create_default_transport::<P>();
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
        let (_, mut dialer) = create_default_transport::<P>();

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
            .take(completed_streams)
            .collect::<Vec<_>>(),
        Duration::from_secs(30),
    );

    quickcheck::TestResult::passed()
}

async fn answer_inbound_streams<P: Provider + Spawn, const BUFFER_SIZE: usize>(
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

async fn open_outbound_streams<P: Provider + Spawn, const BUFFER_SIZE: usize>(
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
#[allow(unknown_lints, clippy::needless_pass_by_ref_mut)] // False positive.
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

trait Spawn {
    /// Run the given future in the background until it ends.
    fn spawn(future: impl Future<Output = ()> + Send + 'static);
}

#[cfg(feature = "async-std")]
impl Spawn for libp2p_quic::async_std::Provider {
    fn spawn(future: impl Future<Output = ()> + Send + 'static) {
        async_std::task::spawn(future);
    }
}

#[cfg(feature = "tokio")]
impl Spawn for libp2p_quic::tokio::Provider {
    fn spawn(future: impl Future<Output = ()> + Send + 'static) {
        tokio::spawn(future);
    }
}
