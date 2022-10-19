use crate::future::{BoxFuture, Either, FutureExt};
use futures::{future, AsyncRead, AsyncWrite};
use futures::{AsyncReadExt, Stream};
use futures::{AsyncWriteExt, StreamExt};
use libp2p_core::multiaddr::Protocol;
use libp2p_core::muxing::StreamMuxerExt;
use libp2p_core::transport::memory::Channel;
use libp2p_core::transport::MemoryTransport;
use libp2p_core::{
    upgrade, InboundUpgrade, Negotiated, OutboundUpgrade, StreamMuxer, Transport, UpgradeInfo,
};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{fmt, mem};

pub async fn connected_muxers_on_memory_transport<MC, M, E>() -> (M, M)
where
    MC: InboundUpgrade<Negotiated<Channel<Vec<u8>>>, Error = E, Output = M>
        + OutboundUpgrade<Negotiated<Channel<Vec<u8>>>, Error = E, Output = M>
        + Send
        + 'static
        + Default,
    <MC as UpgradeInfo>::Info: Send,
    <<MC as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
    <MC as InboundUpgrade<Negotiated<Channel<Vec<u8>>>>>::Future: Send,
    <MC as OutboundUpgrade<Negotiated<Channel<Vec<u8>>>>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut alice = MemoryTransport::default()
        .and_then(move |c, e| upgrade::apply(c, MC::default(), e, upgrade::Version::V1))
        .boxed();
    let mut bob = MemoryTransport::default()
        .and_then(move |c, e| upgrade::apply(c, MC::default(), e, upgrade::Version::V1))
        .boxed();

    alice.listen_on(Protocol::Memory(0).into()).unwrap();
    let listen_address = alice.next().await.unwrap().into_new_address().unwrap();

    futures::future::join(
        async {
            alice
                .next()
                .await
                .unwrap()
                .into_incoming()
                .unwrap()
                .0
                .await
                .unwrap()
        },
        async { bob.dial(listen_address).unwrap().await.unwrap() },
    )
    .await
}

/// Verifies that Alice can send a message and immediately close the stream afterwards and Bob can use `read_to_end` to read the entire message.
pub async fn close_implies_flush<A, B, S, E>(alice: A, bob: B)
where
    A: StreamMuxer<Substream = S, Error = E> + Unpin,
    B: StreamMuxer<Substream = S, Error = E> + Unpin,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    E: fmt::Debug,
{
    run_commutative(
        alice,
        bob,
        |mut stream| async move {
            stream.write_all(b"PING").await.unwrap();
            stream.close().await.unwrap();
        },
        |mut stream| async move {
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();

            assert_eq!(buf, b"PING");
        },
    )
    .await;
}

/// Verifies that the dialer of a substream can receive a message.
pub async fn dialer_can_receive<A, B, S, E>(alice: A, bob: B)
where
    A: StreamMuxer<Substream = S, Error = E> + Unpin,
    B: StreamMuxer<Substream = S, Error = E> + Unpin,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    E: fmt::Debug,
{
    run_commutative(
        alice,
        bob,
        |mut stream| async move {
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();

            assert_eq!(buf, b"PING");
        },
        |mut stream| async move {
            stream.write_all(b"PING").await.unwrap();
            stream.close().await.unwrap();
        },
    )
    .await;
}

/// Verifies that we can "half-close" a substream.
pub async fn read_after_close<A, B, S, E>(alice: A, bob: B)
where
    A: StreamMuxer<Substream = S, Error = E> + Unpin,
    B: StreamMuxer<Substream = S, Error = E> + Unpin,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    E: fmt::Debug,
{
    run_commutative(
        alice,
        bob,
        |mut stream| async move {
            stream.write_all(b"PING").await.unwrap();
            stream.close().await.unwrap();

            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();

            assert_eq!(buf, b"PONG");
        },
        |mut stream| async move {
            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).await.unwrap();

            assert_eq!(&buf, b"PING");

            stream.write_all(b"PONG").await.unwrap();
            stream.close().await.unwrap();
        },
    )
    .await;
}

/// Runs the given protocol between the two parties, ensuring commutativity, i.e. either party can be the dialer and listener.
async fn run_commutative<A, B, S, E, F1, F2>(
    mut alice: A,
    mut bob: B,
    alice_proto: impl Fn(S) -> F1 + Clone + 'static,
    bob_proto: impl Fn(S) -> F2 + Clone + 'static,
) where
    A: StreamMuxer<Substream = S, Error = E> + Unpin,
    B: StreamMuxer<Substream = S, Error = E> + Unpin,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    E: fmt::Debug,
    F1: Future<Output = ()> + Send + 'static,
    F2: Future<Output = ()> + Send + 'static,
{
    run(&mut alice, &mut bob, alice_proto.clone(), bob_proto.clone()).await;
    run(&mut bob, &mut alice, alice_proto, bob_proto).await;
}

/// Runs a given protocol between the two parties.
///
/// The first party will open a new substream and the second party will wait for this.
/// The [`StreamMuxer`] is polled until both parties have completed the protocol to ensure that the underlying connection can make progress at all times.
async fn run<A, B, S, E, F1, F2>(
    dialer: &mut A,
    listener: &mut B,
    alice_proto: impl Fn(S) -> F1 + 'static,
    bob_proto: impl Fn(S) -> F2 + 'static,
) where
    A: StreamMuxer<Substream = S, Error = E> + Unpin,
    B: StreamMuxer<Substream = S, Error = E> + Unpin,
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    E: fmt::Debug,
    F1: Future<Output = ()> + Send + 'static,
    F2: Future<Output = ()> + Send + 'static,
{
    let mut dialer = Harness::OutboundSetup {
        muxer: dialer,
        proto_fn: Box::new(move |s| alice_proto(s).boxed()),
    };
    let mut listener = Harness::InboundSetup {
        muxer: listener,
        proto_fn: Box::new(move |s| bob_proto(s).boxed()),
    };

    let mut dialer_complete = false;
    let mut listener_complete = false;

    loop {
        match futures::future::select(dialer.next(), listener.next()).await {
            Either::Left((Some(Event::SetupComplete), _)) => {
                log::info!("Dialer opened outbound stream");
            }
            Either::Left((Some(Event::ProtocolComplete), _)) => {
                log::info!("Dialer completed protocol");
                dialer_complete = true
            }
            Either::Left((Some(Event::Timeout), _)) => {
                panic!("Dialer protocol timed out");
            }
            Either::Right((Some(Event::SetupComplete), _)) => {
                log::info!("Listener received inbound stream");
            }
            Either::Right((Some(Event::ProtocolComplete), _)) => {
                log::info!("Listener completed protocol");
                listener_complete = true
            }
            Either::Right((Some(Event::Timeout), _)) => {
                panic!("Listener protocol timed out");
            }
            _ => unreachable!(),
        }

        if dialer_complete && listener_complete {
            break;
        }
    }
}

enum Harness<'m, M>
where
    M: StreamMuxer,
{
    InboundSetup {
        muxer: &'m mut M,
        proto_fn: Box<dyn FnOnce(M::Substream) -> BoxFuture<'static, ()>>,
    },
    OutboundSetup {
        muxer: &'m mut M,
        proto_fn: Box<dyn FnOnce(M::Substream) -> BoxFuture<'static, ()>>,
    },
    Running {
        muxer: &'m mut M,
        timeout: futures_timer::Delay,
        proto: BoxFuture<'static, ()>,
    },
    Complete {
        muxer: &'m mut M,
    },
    Poisoned,
}

enum Event {
    SetupComplete,
    Timeout,
    ProtocolComplete,
}

impl<'m, M> Stream for Harness<'m, M>
where
    M: StreamMuxer + Unpin,
{
    type Item = Event;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            match mem::replace(this, Self::Poisoned) {
                Harness::InboundSetup { muxer, proto_fn } => {
                    if let Poll::Ready(stream) = muxer.poll_inbound_unpin(cx) {
                        *this = Harness::Running {
                            muxer,
                            timeout: futures_timer::Delay::new(Duration::from_secs(10)),
                            proto: proto_fn(stream.unwrap()),
                        };
                        return Poll::Ready(Some(Event::SetupComplete));
                    }

                    if let Poll::Ready(event) = muxer.poll_unpin(cx) {
                        event.unwrap();

                        *this = Harness::InboundSetup { muxer, proto_fn };
                        continue;
                    }

                    *this = Harness::InboundSetup { muxer, proto_fn };
                    return Poll::Pending;
                }
                Harness::OutboundSetup { muxer, proto_fn } => {
                    if let Poll::Ready(stream) = muxer.poll_outbound_unpin(cx) {
                        *this = Harness::Running {
                            muxer,
                            timeout: futures_timer::Delay::new(Duration::from_secs(10)),
                            proto: proto_fn(stream.unwrap()),
                        };
                        return Poll::Ready(Some(Event::SetupComplete));
                    }

                    if let Poll::Ready(event) = muxer.poll_unpin(cx) {
                        event.unwrap();

                        *this = Harness::OutboundSetup { muxer, proto_fn };
                        continue;
                    }

                    *this = Harness::OutboundSetup { muxer, proto_fn };
                    return Poll::Pending;
                }
                Harness::Running {
                    muxer,
                    mut proto,
                    mut timeout,
                } => {
                    if let Poll::Ready(event) = muxer.poll_unpin(cx) {
                        event.unwrap();

                        *this = Harness::Running {
                            muxer,
                            proto,
                            timeout,
                        };
                        continue;
                    }

                    if let Poll::Ready(()) = proto.poll_unpin(cx) {
                        *this = Harness::Complete { muxer };
                        return Poll::Ready(Some(Event::ProtocolComplete));
                    }

                    if let Poll::Ready(()) = timeout.poll_unpin(cx) {
                        return Poll::Ready(Some(Event::Timeout));
                    }

                    *this = Harness::Running {
                        muxer,
                        proto,
                        timeout,
                    };
                    return Poll::Pending;
                }
                Harness::Complete { muxer } => {
                    if let Poll::Ready(event) = muxer.poll_unpin(cx) {
                        event.unwrap();

                        *this = Harness::Complete { muxer };
                        continue;
                    }

                    *this = Harness::Complete { muxer };
                    return Poll::Pending;
                }
                Harness::Poisoned => {
                    unreachable!()
                }
            }
        }
    }
}
