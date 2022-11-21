// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use futures::channel::mpsc;
use futures::future::Either;
use futures::stream::StreamExt;
use futures::{future, AsyncReadExt, AsyncWriteExt, SinkExt};
use libp2p_core::muxing::{StreamMuxerBox, StreamMuxerExt};
use libp2p_core::transport::{Boxed, TransportEvent};
use libp2p_core::{Multiaddr, PeerId, Transport};
use libp2p_webrtc as webrtc;
use rand::{thread_rng, RngCore};
use std::io;
use std::num::NonZeroU8;
use std::time::Duration;

#[tokio::test]
async fn smoke() {
    let _ = env_logger::try_init();

    let (a_peer_id, mut a_transport) = create_transport();
    let (b_peer_id, mut b_transport) = create_transport();

    let addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/webrtc").await;
    start_listening(&mut b_transport, "/ip4/127.0.0.1/udp/0/webrtc").await;
    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, addr).await;

    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

// Note: This test should likely be ported to the muxer compliance test suite.
#[test]
fn concurrent_connections_and_streams_tokio() {
    let _ = env_logger::try_init();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    quickcheck::QuickCheck::new()
        .min_tests_passed(1)
        .quickcheck(prop as fn(_, _) -> _);
}

fn generate_tls_keypair() -> libp2p_core::identity::Keypair {
    libp2p_core::identity::Keypair::generate_ed25519()
}

fn create_transport() -> (PeerId, Boxed<(PeerId, StreamMuxerBox)>) {
    let keypair = generate_tls_keypair();
    let peer_id = keypair.public().to_peer_id();

    let transport = webrtc::tokio::Transport::new(
        keypair,
        webrtc::tokio::Certificate::generate(&mut thread_rng()).unwrap(),
    )
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

fn prop(number_listeners: NonZeroU8, number_streams: NonZeroU8) -> quickcheck::TestResult {
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
        tokio::spawn({
            let mut listeners_tx = listeners_tx.clone();

            async move {
                let (peer_id, mut listener) = create_transport();
                let addr = start_listening(&mut listener, "/ip4/127.0.0.1/udp/0/webrtc").await;

                listeners_tx.send((peer_id, addr)).await.unwrap();

                loop {
                    if let TransportEvent::Incoming { upgrade, .. } =
                        listener.select_next_some().await
                    {
                        let (_, connection) = upgrade.await.unwrap();

                        tokio::spawn(answer_inbound_streams::<BUFFER_SIZE>(connection));
                    }
                }
            }
        });
    }

    let (completed_streams_tx, completed_streams_rx) =
        mpsc::channel(number_streams * number_listeners);

    // For each listener node start `number_streams` requests.
    tokio::spawn(async move {
        let (_, mut dialer) = create_transport();

        while let Some((_, listener_addr)) = listeners_rx.next().await {
            let (_, connection) = dial(&mut dialer, listener_addr.clone()).await.unwrap();

            tokio::spawn(open_outbound_streams::<BUFFER_SIZE>(
                connection,
                number_streams,
                completed_streams_tx.clone(),
            ));
        }

        // Drive the dialer.
        loop {
            dialer.next().await;
        }
    });

    let completed_streams = number_streams * number_listeners;

    // Wait for all streams to complete.
    tokio::runtime::Handle::current()
        .block_on(tokio::time::timeout(
            Duration::from_secs(30),
            completed_streams_rx
                .take(completed_streams)
                .collect::<Vec<_>>(),
        ))
        .unwrap();

    quickcheck::TestResult::passed()
}

async fn answer_inbound_streams<const BUFFER_SIZE: usize>(mut connection: StreamMuxerBox) {
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

        tokio::spawn(async move {
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

async fn open_outbound_streams<const BUFFER_SIZE: usize>(
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

        tokio::spawn({
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
            loop {
                if let Some((upgrade, send_back_addr)) =
                    listener.select_next_some().await.into_incoming()
                {
                    let (peer_id, connection) = upgrade.await.unwrap();

                    return (peer_id, send_back_addr, connection);
                }
            }
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
