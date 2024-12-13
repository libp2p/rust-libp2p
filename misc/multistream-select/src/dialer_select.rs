// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Protocol negotiation strategies for the peer acting as the dialer.

use std::{
    convert::TryFrom as _,
    iter, mem,
    pin::Pin,
    task::{Context, Poll},
};

use futures::prelude::*;

use crate::{
    protocol::{HeaderLine, Message, MessageIO, Protocol, ProtocolError},
    Negotiated, NegotiationError, Version,
};

/// Returns a `Future` that negotiates a protocol on the given I/O stream
/// for a peer acting as the _dialer_ (or _initiator_).
///
/// This function is given an I/O stream and a list of protocols and returns a
/// computation that performs the protocol negotiation with the remote. The
/// returned `Future` resolves with the name of the negotiated protocol and
/// a [`Negotiated`] I/O stream.
///
/// Within the scope of this library, a dialer always commits to a specific
/// multistream-select [`Version`], whereas a listener always supports
/// all versions supported by this library. Frictionless multistream-select
/// protocol upgrades may thus proceed by deployments with updated listeners,
/// eventually followed by deployments of dialers choosing the newer protocol.
pub fn dialer_select_proto<R, I>(
    inner: R,
    protocols: I,
    version: Version,
) -> DialerSelectFuture<R, I::IntoIter>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let protocols = protocols.into_iter().peekable();
    DialerSelectFuture {
        version,
        protocols,
        state: State::SendHeader {
            io: MessageIO::new(inner),
        },
    }
}

/// A `Future` returned by [`dialer_select_proto`] which negotiates
/// a protocol iteratively by considering one protocol after the other.
#[pin_project::pin_project]
pub struct DialerSelectFuture<R, I: Iterator> {
    // TODO: It would be nice if eventually N = I::Item = Protocol.
    protocols: iter::Peekable<I>,
    state: State<R, I::Item>,
    version: Version,
}

enum State<R, N> {
    SendHeader { io: MessageIO<R> },
    SendProtocol { io: MessageIO<R>, protocol: N },
    FlushProtocol { io: MessageIO<R>, protocol: N },
    AwaitProtocol { io: MessageIO<R>, protocol: N },
    Done,
}

impl<R, I> Future for DialerSelectFuture<R, I>
where
    // The Unpin bound here is required because we produce
    // a `Negotiated<R>` as the output. It also makes
    // the implementation considerably easier to write.
    R: AsyncRead + AsyncWrite + Unpin,
    I: Iterator,
    I::Item: AsRef<str>,
{
    type Output = Result<(I::Item, Negotiated<R>), NegotiationError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        loop {
            match mem::replace(this.state, State::Done) {
                State::SendHeader { mut io } => {
                    match Pin::new(&mut io).poll_ready(cx)? {
                        Poll::Ready(()) => {}
                        Poll::Pending => {
                            *this.state = State::SendHeader { io };
                            return Poll::Pending;
                        }
                    }

                    let h = HeaderLine::from(*this.version);
                    if let Err(err) = Pin::new(&mut io).start_send(Message::Header(h)) {
                        return Poll::Ready(Err(From::from(err)));
                    }

                    let protocol = this.protocols.next().ok_or(NegotiationError::Failed)?;

                    // The dialer always sends the header and the first protocol
                    // proposal in one go for efficiency.
                    *this.state = State::SendProtocol { io, protocol };
                }

                State::SendProtocol { mut io, protocol } => {
                    match Pin::new(&mut io).poll_ready(cx)? {
                        Poll::Ready(()) => {}
                        Poll::Pending => {
                            *this.state = State::SendProtocol { io, protocol };
                            return Poll::Pending;
                        }
                    }

                    let p = Protocol::try_from(protocol.as_ref())?;
                    if let Err(err) = Pin::new(&mut io).start_send(Message::Protocol(p.clone())) {
                        return Poll::Ready(Err(From::from(err)));
                    }
                    tracing::debug!(protocol=%p, "Dialer: Proposed protocol");

                    if this.protocols.peek().is_some() {
                        *this.state = State::FlushProtocol { io, protocol }
                    } else {
                        match this.version {
                            Version::V1 => *this.state = State::FlushProtocol { io, protocol },
                            // This is the only effect that `V1Lazy` has compared to `V1`:
                            // Optimistically settling on the only protocol that
                            // the dialer supports for this negotiation. Notably,
                            // the dialer expects a regular `V1` response.
                            Version::V1Lazy => {
                                tracing::debug!(protocol=%p, "Dialer: Expecting proposed protocol");
                                let hl = HeaderLine::from(Version::V1Lazy);
                                let io = Negotiated::expecting(io.into_reader(), p, Some(hl));
                                return Poll::Ready(Ok((protocol, io)));
                            }
                        }
                    }
                }

                State::FlushProtocol { mut io, protocol } => {
                    match Pin::new(&mut io).poll_flush(cx)? {
                        Poll::Ready(()) => *this.state = State::AwaitProtocol { io, protocol },
                        Poll::Pending => {
                            *this.state = State::FlushProtocol { io, protocol };
                            return Poll::Pending;
                        }
                    }
                }

                State::AwaitProtocol { mut io, protocol } => {
                    let msg = match Pin::new(&mut io).poll_next(cx)? {
                        Poll::Ready(Some(msg)) => msg,
                        Poll::Pending => {
                            *this.state = State::AwaitProtocol { io, protocol };
                            return Poll::Pending;
                        }
                        // Treat EOF error as [`NegotiationError::Failed`], not as
                        // [`NegotiationError::ProtocolError`], allowing dropping or closing an I/O
                        // stream as a permissible way to "gracefully" fail a negotiation.
                        Poll::Ready(None) => return Poll::Ready(Err(NegotiationError::Failed)),
                    };

                    match msg {
                        Message::Header(v) if v == HeaderLine::from(*this.version) => {
                            *this.state = State::AwaitProtocol { io, protocol };
                        }
                        Message::Protocol(ref p) if p.as_ref() == protocol.as_ref() => {
                            tracing::debug!(protocol=%p, "Dialer: Received confirmation for protocol");
                            let io = Negotiated::completed(io.into_inner());
                            return Poll::Ready(Ok((protocol, io)));
                        }
                        Message::NotAvailable => {
                            tracing::debug!(
                                protocol=%protocol.as_ref(),
                                "Dialer: Received rejection of protocol"
                            );
                            let protocol = this.protocols.next().ok_or(NegotiationError::Failed)?;
                            *this.state = State::SendProtocol { io, protocol }
                        }
                        _ => return Poll::Ready(Err(ProtocolError::InvalidMessage.into())),
                    }
                }

                State::Done => panic!("State::poll called after completion"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use async_std::{
        future::timeout,
        net::{TcpListener, TcpStream},
    };
    use quickcheck::{Arbitrary, Gen, GenRange};
    use tracing::metadata::LevelFilter;
    use tracing_subscriber::EnvFilter;

    use super::*;
    use crate::listener_select_proto;

    #[test]
    fn select_proto_basic() {
        async fn run(version: Version) {
            let (client_connection, server_connection) = futures_ringbuf::Endpoint::pair(100, 100);

            let server = async_std::task::spawn(async move {
                let protos = vec!["/proto1", "/proto2"];
                let (proto, mut io) = listener_select_proto(server_connection, protos)
                    .await
                    .unwrap();
                assert_eq!(proto, "/proto2");

                let mut out = vec![0; 32];
                let n = io.read(&mut out).await.unwrap();
                out.truncate(n);
                assert_eq!(out, b"ping");

                io.write_all(b"pong").await.unwrap();
                io.flush().await.unwrap();
            });

            let client = async_std::task::spawn(async move {
                let protos = vec!["/proto3", "/proto2"];
                let (proto, mut io) = dialer_select_proto(client_connection, protos, version)
                    .await
                    .unwrap();
                assert_eq!(proto, "/proto2");

                io.write_all(b"ping").await.unwrap();
                io.flush().await.unwrap();

                let mut out = vec![0; 32];
                let n = io.read(&mut out).await.unwrap();
                out.truncate(n);
                assert_eq!(out, b"pong");
            });

            server.await;
            client.await;
        }

        async_std::task::block_on(run(Version::V1));
        async_std::task::block_on(run(Version::V1Lazy));
    }

    /// Tests the expected behaviour of failed negotiations.
    #[test]
    fn negotiation_failed() {
        fn prop(
            version: Version,
            DialerProtos(dial_protos): DialerProtos,
            ListenerProtos(listen_protos): ListenerProtos,
            DialPayload(dial_payload): DialPayload,
        ) {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(
                    EnvFilter::builder()
                        .with_default_directive(LevelFilter::DEBUG.into())
                        .from_env_lossy(),
                )
                .try_init();

            async_std::task::block_on(async move {
                let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
                let addr = listener.local_addr().unwrap();

                let server = async_std::task::spawn(async move {
                    let server_connection = listener.accept().await.unwrap().0;

                    let io = match timeout(
                        Duration::from_secs(2),
                        listener_select_proto(server_connection, listen_protos),
                    )
                    .await
                    .unwrap()
                    {
                        Ok((_, io)) => io,
                        Err(NegotiationError::Failed) => return,
                        Err(NegotiationError::ProtocolError(e)) => {
                            panic!("Unexpected protocol error {e}")
                        }
                    };
                    match io.complete().await {
                        Err(NegotiationError::Failed) => {}
                        _ => panic!(),
                    }
                });

                let client = async_std::task::spawn(async move {
                    let client_connection = TcpStream::connect(addr).await.unwrap();

                    let mut io = match timeout(
                        Duration::from_secs(2),
                        dialer_select_proto(client_connection, dial_protos, version),
                    )
                    .await
                    .unwrap()
                    {
                        Err(NegotiationError::Failed) => return,
                        Ok((_, io)) => io,
                        Err(_) => panic!(),
                    };
                    // The dialer may write a payload that is even sent before it
                    // got confirmation of the last proposed protocol, when `V1Lazy`
                    // is used.

                    tracing::info!("Writing early data");

                    io.write_all(&dial_payload).await.unwrap();
                    match io.complete().await {
                        Err(NegotiationError::Failed) => {}
                        _ => panic!(),
                    }
                });

                server.await;
                client.await;

                tracing::info!("---------------------------------------")
            });
        }

        quickcheck::QuickCheck::new()
            .tests(1000)
            .quickcheck(prop as fn(_, _, _, _));
    }

    #[async_std::test]
    async fn v1_lazy_do_not_wait_for_negotiation_on_poll_close() {
        let (client_connection, _server_connection) =
            futures_ringbuf::Endpoint::pair(1024 * 1024, 1);

        let client = async_std::task::spawn(async move {
            // Single protocol to allow for lazy (or optimistic) protocol negotiation.
            let protos = vec!["/proto1"];
            let (proto, mut io) = dialer_select_proto(client_connection, protos, Version::V1Lazy)
                .await
                .unwrap();
            assert_eq!(proto, "/proto1");

            // client can close the connection even though protocol negotiation is not yet done,
            // i.e. `_server_connection` had been untouched.
            io.close().await.unwrap();
        });

        async_std::future::timeout(Duration::from_secs(10), client)
            .await
            .unwrap();
    }

    #[derive(Clone, Debug)]
    struct DialerProtos(Vec<&'static str>);

    impl Arbitrary for DialerProtos {
        fn arbitrary(g: &mut Gen) -> Self {
            if bool::arbitrary(g) {
                DialerProtos(vec!["/proto1"])
            } else {
                DialerProtos(vec!["/proto1", "/proto2"])
            }
        }
    }

    #[derive(Clone, Debug)]
    struct ListenerProtos(Vec<&'static str>);

    impl Arbitrary for ListenerProtos {
        fn arbitrary(g: &mut Gen) -> Self {
            if bool::arbitrary(g) {
                ListenerProtos(vec!["/proto3"])
            } else {
                ListenerProtos(vec!["/proto3", "/proto4"])
            }
        }
    }

    #[derive(Clone, Debug)]
    struct DialPayload(Vec<u8>);

    impl Arbitrary for DialPayload {
        fn arbitrary(g: &mut Gen) -> Self {
            DialPayload(
                (0..g.gen_range(0..2u8))
                    .map(|_| g.gen_range(1..255)) // We can generate 0 as that will produce a different error.
                    .collect(),
            )
        }
    }
}
