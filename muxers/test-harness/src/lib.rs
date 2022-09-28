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
    run(
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

/// Runs the given protocol between the two parties.
///
/// The protocol always starts with Alice opening a substream to Bob.
/// After that, both muxers are polled until both parties complete the protocol.
///
/// Esp. the latter is important. The contract of [`StreamMuxer`] states that we must call [`StreamMuxer::poll`]
/// for the underlying connection to make progress.
async fn run<A, B, S, E, F1, F2>(
    alice: A,
    bob: B,
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
    let mut alice = Harness::OutboundSetup {
        muxer: alice,
        proto_fn: Box::new(move |s| alice_proto(s).boxed()),
    };
    let mut bob = Harness::InboundSetup {
        muxer: bob,
        proto_fn: Box::new(move |s| bob_proto(s).boxed()),
    };

    let mut alice_complete = false;
    let mut bob_complete = false;

    loop {
        match futures::future::select(alice.next(), bob.next()).await {
            Either::Left((Some(Event::SetupComplete), _)) => {
                log::info!("Alice opened outbound stream");
            }
            Either::Left((Some(Event::ProtocolComplete), _)) => {
                log::info!("Alice completed protocol");
                alice_complete = true
            }
            Either::Right((Some(Event::SetupComplete), _)) => {
                log::info!("Bob received inbound stream");
            }
            Either::Right((Some(Event::ProtocolComplete), _)) => {
                log::info!("Bob completed protocol");
                bob_complete = true
            }
            _ => unreachable!(),
        }

        if alice_complete && bob_complete {
            break;
        }
    }
}

enum Harness<M>
where
    M: StreamMuxer,
{
    InboundSetup {
        muxer: M,
        proto_fn: Box<dyn FnOnce(M::Substream) -> BoxFuture<'static, ()>>,
    },
    OutboundSetup {
        muxer: M,
        proto_fn: Box<dyn FnOnce(M::Substream) -> BoxFuture<'static, ()>>,
    },
    Running {
        muxer: M,
        proto: BoxFuture<'static, ()>,
    },
    Complete {
        muxer: M,
    },
    Poisoned,
}

enum Event {
    SetupComplete,
    ProtocolComplete,
}

impl<M> Stream for Harness<M>
where
    M: StreamMuxer + Unpin,
{
    type Item = Event;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            match mem::replace(this, Self::Poisoned) {
                Harness::InboundSetup {
                    mut muxer,
                    proto_fn,
                } => {
                    if let Poll::Ready(stream) = muxer.poll_inbound_unpin(cx) {
                        *this = Harness::Running {
                            muxer,
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
                Harness::OutboundSetup {
                    mut muxer,
                    proto_fn,
                } => {
                    if let Poll::Ready(stream) = muxer.poll_outbound_unpin(cx) {
                        *this = Harness::Running {
                            muxer,
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
                    mut muxer,
                    mut proto,
                } => {
                    if let Poll::Ready(event) = muxer.poll_unpin(cx) {
                        event.unwrap();

                        *this = Harness::Running { muxer, proto };
                        continue;
                    }

                    if let Poll::Ready(()) = proto.poll_unpin(cx) {
                        *this = Harness::Complete { muxer };
                        return Poll::Ready(Some(Event::ProtocolComplete));
                    }

                    *this = Harness::Running { muxer, proto };
                    return Poll::Pending;
                }
                Harness::Complete { mut muxer } => {
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
