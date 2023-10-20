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

use crate::protocol::{HeaderLine, Message, MessageIO, Protocol, ProtocolError};
use crate::{Negotiated, NegotiationError, Version};

use futures::prelude::*;
use std::{
    convert::TryFrom as _,
    iter, mem,
    pin::Pin,
    task::{Context, Poll},
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
    // The Unpin bound here is required because we produce a `Negotiated<R>` as the output.
    // It also makes the implementation considerably easier to write.
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
                    log::debug!("Dialer: Proposed protocol: {}", p);

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
                                log::debug!("Dialer: Expecting proposed protocol: {}", p);
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
                            log::debug!("Dialer: Received confirmation for protocol: {}", p);
                            let io = Negotiated::completed(io.into_inner());
                            return Poll::Ready(Ok((protocol, io)));
                        }
                        Message::NotAvailable => {
                            log::debug!(
                                "Dialer: Received rejection of protocol: {}",
                                protocol.as_ref()
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
    use super::*;
    use crate::listener_select_proto;
    use std::time::Duration;

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
        let _ = env_logger::try_init();

        async fn run(
            Test {
                version,
                listen_protos,
                dial_protos,
                dial_payload,
            }: Test,
        ) {
            let (client_connection, server_connection) = futures_ringbuf::Endpoint::pair(100, 100);

            let server = async_std::task::spawn(async move {
                let io = match listener_select_proto(server_connection, listen_protos).await {
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
                let mut io =
                    match dialer_select_proto(client_connection, dial_protos, version).await {
                        Err(NegotiationError::Failed) => return,
                        Ok((_, io)) => io,
                        Err(_) => panic!(),
                    };
                // The dialer may write a payload that is even sent before it
                // got confirmation of the last proposed protocol, when `V1Lazy`
                // is used.
                io.write_all(&dial_payload).await.unwrap();
                match io.complete().await {
                    Err(NegotiationError::Failed) => {}
                    _ => panic!(),
                }
            });

            server.await;
            client.await;
        }

        /// Parameters for a single test run.
        #[derive(Clone)]
        struct Test {
            version: Version,
            listen_protos: Vec<&'static str>,
            dial_protos: Vec<&'static str>,
            dial_payload: Vec<u8>,
        }

        // Disjunct combinations of listen and dial protocols to test.
        //
        // The choices here cover the main distinction between a single
        // and multiple protocols.
        let protos = vec![
            (vec!["/proto1"], vec!["/proto2"]),
            (vec!["/proto1", "/proto2"], vec!["/proto3", "/proto4"]),
        ];

        // The payloads that the dialer sends after "successful" negotiation,
        // which may be sent even before the dialer got protocol confirmation
        // when `V1Lazy` is used.
        //
        // The choices here cover the specific situations that can arise with
        // `V1Lazy` and which must nevertheless behave identically to `V1` w.r.t.
        // the outcome of the negotiation.
        let payloads = vec![
            // No payload, in which case all versions should behave identically
            // in any case, i.e. the baseline test.
            vec![],
            // With this payload and `V1Lazy`, the listener interprets the first
            // `1` as a message length and encounters an invalid message (the
            // second `1`). The listener is nevertheless expected to fail
            // negotiation normally, just like with `V1`.
            vec![1, 1],
            // With this payload and `V1Lazy`, the listener interprets the first
            // `42` as a message length and encounters unexpected EOF trying to
            // read a message of that length. The listener is nevertheless expected
            // to fail negotiation normally, just like with `V1`
            vec![42, 1],
        ];

        for (listen_protos, dial_protos) in protos {
            for dial_payload in payloads.clone() {
                for &version in &[Version::V1, Version::V1Lazy] {
                    async_std::task::block_on(run(Test {
                        version,
                        listen_protos: listen_protos.clone(),
                        dial_protos: dial_protos.clone(),
                        dial_payload: dial_payload.clone(),
                    }))
                }
            }
        }
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

            // client can close the connection even though protocol negotiation is not yet done, i.e.
            // `_server_connection` had been untouched.
            io.close().await.unwrap();
        });

        async_std::future::timeout(Duration::from_secs(10), client)
            .await
            .unwrap();
    }
}
