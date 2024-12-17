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

use std::{
    future::Future,
    num::NonZeroU8,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    channel::mpsc,
    future,
    future::{BoxFuture, Either},
    ready,
    stream::StreamExt,
    AsyncReadExt, AsyncWriteExt, FutureExt, SinkExt,
};
use libp2p_core::{
    muxing::{StreamMuxerBox, StreamMuxerExt},
    transport::{Boxed, DialOpts, ListenerId, PortUse, TransportEvent},
    Endpoint, Multiaddr, Transport,
};
use libp2p_identity::PeerId;
use libp2p_webrtc as webrtc;
use rand::{thread_rng, RngCore};
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn smoke() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (a_peer_id, mut a_transport) = create_transport();
    let (b_peer_id, mut b_transport) = create_transport();

    let addr = start_listening(&mut a_transport, "/ip4/127.0.0.1/udp/0/webrtc-direct").await;
    start_listening(&mut b_transport, "/ip4/127.0.0.1/udp/0/webrtc-direct").await;
    let ((a_connected, _, _), (b_connected, _)) =
        connect(&mut a_transport, &mut b_transport, addr).await;

    assert_eq!(a_connected, b_peer_id);
    assert_eq!(b_connected, a_peer_id);
}

// Note: This test should likely be ported to the muxer compliance test suite.
#[test]
fn concurrent_connections_and_streams_tokio() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    quickcheck::QuickCheck::new()
        .min_tests_passed(1)
        .quickcheck(prop as fn(_, _) -> _);
}

fn generate_tls_keypair() -> libp2p_identity::Keypair {
    libp2p_identity::Keypair::generate_ed25519()
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
    transport
        .listen_on(ListenerId::next(), addr.parse().unwrap())
        .unwrap();
    match transport.next().await {
        Some(TransportEvent::NewAddress { listen_addr, .. }) => listen_addr,
        e => panic!("{e:?}"),
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

    tracing::info!(
        stream_count=%number_streams,
        connection_count=%number_listeners,
        "Creating streams on connections"
    );

    // Spawn the listener nodes.
    for _ in 0..number_listeners {
        tokio::spawn({
            let mut listeners_tx = listeners_tx.clone();

            async move {
                let (peer_id, mut listener) = create_transport();
                let addr =
                    start_listening(&mut listener, "/ip4/127.0.0.1/udp/0/webrtc-direct").await;

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
            let (_, connection) = Dial::new(&mut dialer, listener_addr.clone()).await;

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
        let Ok(mut inbound_stream) = future::poll_fn(|cx| {
            let _ = connection.poll_unpin(cx)?;
            connection.poll_inbound_unpin(cx)
        })
        .await
        else {
            return;
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

    tracing::info!(stream_count=%number_streams, "Created streams");

    while future::poll_fn(|cx| connection.poll_unpin(cx))
        .await
        .is_ok()
    {}
}

async fn connect(
    a_transport: &mut Boxed<(PeerId, StreamMuxerBox)>,
    b_transport: &mut Boxed<(PeerId, StreamMuxerBox)>,
    addr: Multiaddr,
) -> (
    (PeerId, Multiaddr, StreamMuxerBox),
    (PeerId, StreamMuxerBox),
) {
    match futures::future::select(
        ListenUpgrade::new(a_transport),
        Dial::new(b_transport, addr),
    )
    .await
    {
        Either::Left((listen_done, dial)) => {
            let mut pending_dial = dial;

            loop {
                match future::select(pending_dial, a_transport.next()).await {
                    Either::Left((dial_done, _)) => return (listen_done, dial_done),
                    Either::Right((_, dial)) => {
                        pending_dial = dial;
                    }
                }
            }
        }
        Either::Right((dial_done, listen)) => {
            let mut pending_listen = listen;

            loop {
                match future::select(pending_listen, b_transport.next()).await {
                    Either::Left((listen_done, _)) => return (listen_done, dial_done),
                    Either::Right((_, listen)) => {
                        pending_listen = listen;
                    }
                }
            }
        }
    }
}

struct ListenUpgrade<'a> {
    listener: &'a mut Boxed<(PeerId, StreamMuxerBox)>,
    listener_upgrade_task: Option<BoxFuture<'static, (PeerId, Multiaddr, StreamMuxerBox)>>,
}

impl<'a> ListenUpgrade<'a> {
    pub(crate) fn new(listener: &'a mut Boxed<(PeerId, StreamMuxerBox)>) -> Self {
        Self {
            listener,
            listener_upgrade_task: None,
        }
    }
}

struct Dial<'a> {
    dialer: &'a mut Boxed<(PeerId, StreamMuxerBox)>,
    dial_task: BoxFuture<'static, (PeerId, StreamMuxerBox)>,
}

impl<'a> Dial<'a> {
    fn new(dialer: &'a mut Boxed<(PeerId, StreamMuxerBox)>, addr: Multiaddr) -> Self {
        Self {
            dial_task: dialer
                .dial(
                    addr,
                    DialOpts {
                        role: Endpoint::Dialer,
                        port_use: PortUse::Reuse,
                    },
                )
                .unwrap()
                .map(|r| r.unwrap())
                .boxed(),
            dialer,
        }
    }
}

impl Future for Dial<'_> {
    type Output = (PeerId, StreamMuxerBox);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.dialer.poll_next_unpin(cx) {
                Poll::Ready(_) => {
                    continue;
                }
                Poll::Pending => {}
            }

            let conn = ready!(self.dial_task.poll_unpin(cx));
            return Poll::Ready(conn);
        }
    }
}

impl Future for ListenUpgrade<'_> {
    type Output = (PeerId, Multiaddr, StreamMuxerBox);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.listener.poll_next_unpin(cx) {
                Poll::Ready(Some(TransportEvent::Incoming {
                    upgrade,
                    send_back_addr,
                    ..
                })) => {
                    self.listener_upgrade_task = Some(
                        async move {
                            let (peer, conn) = upgrade.await.unwrap();

                            (peer, send_back_addr, conn)
                        }
                        .boxed(),
                    );
                    continue;
                }
                Poll::Ready(None) => unreachable!("stream never ends"),
                Poll::Ready(Some(_)) => continue,
                Poll::Pending => {}
            }

            let conn = match self.listener_upgrade_task.as_mut() {
                None => return Poll::Pending,
                Some(inner) => ready!(inner.poll_unpin(cx)),
            };

            return Poll::Ready(conn);
        }
    }
}
