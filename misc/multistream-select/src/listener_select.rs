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

//! Protocol negotiation strategies for the peer acting as the listener
//! in a multistream-select protocol negotiation.

use crate::{Negotiated, NegotiationError};
use crate::protocol::{Protocol, ProtocolError, MessageIO, Message, Version};

use futures::prelude::*;
use smallvec::SmallVec;
use std::{convert::TryFrom as _, io, iter::FromIterator, mem, pin::Pin, task::{Context, Poll}};

/// Returns a `Future` that negotiates a protocol on the given I/O stream
/// for a peer acting as the _listener_ (or _responder_).
///
/// This function is given an I/O stream and a list of protocols and returns a
/// computation that performs the protocol negotiation with the remote. The
/// returned `Future` resolves with the name of the negotiated protocol and
/// a [`Negotiated`] I/O stream.
pub fn listener_select_proto<R, I>(
    inner: R,
    protocols: I,
) -> ListenerSelectFuture<R, I::Item>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let protocols = protocols.into_iter().filter_map(|n|
        match Protocol::try_from(n.as_ref()) {
            Ok(p) => Some((n, p)),
            Err(e) => {
                log::warn!("Listener: Ignoring invalid protocol: {} due to {}",
                      String::from_utf8_lossy(n.as_ref()), e);
                None
            }
        });
    ListenerSelectFuture {
        protocols: SmallVec::from_iter(protocols),
        state: State::RecvHeader {
            io: MessageIO::new(inner)
        }
    }
}

/// The `Future` returned by [`listener_select_proto`] that performs a
/// multistream-select protocol negotiation on an underlying I/O stream.
#[pin_project::pin_project]
pub struct ListenerSelectFuture<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    // TODO: It would be nice if eventually N = Protocol, which has a
    // few more implications on the API.
    protocols: SmallVec<[(N, Protocol); 8]>,
    state: State<R, N>
}

enum State<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    RecvHeader { io: MessageIO<R> },
    SendHeader { io: MessageIO<R>, version: Version },
    RecvMessage { io: MessageIO<R> },
    SendMessage {
        io: MessageIO<R>,
        message: Message,
        protocol: Option<N>
    },
    Flush { io: MessageIO<R> },
    Done
}

impl<R, N> Future for ListenerSelectFuture<R, N>
where
    // The Unpin bound here is required because we produce a `Negotiated<R>` as the output.
    // It also makes the implementation considerably easier to write.
    R: AsyncRead + AsyncWrite + Unpin,
    N: AsRef<[u8]> + Clone
{
    type Output = Result<(N, Negotiated<R>), NegotiationError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        loop {
            match mem::replace(this.state, State::Done) {
                State::RecvHeader { mut io } => {
                    match io.poll_next_unpin(cx) {
                        Poll::Ready(Some(Ok(Message::Header(version)))) => {
                            *this.state = State::SendHeader { io, version }
                        }
                        Poll::Ready(Some(Ok(_))) => {
                            return Poll::Ready(Err(ProtocolError::InvalidMessage.into()))
                        },
                        Poll::Ready(Some(Err(err))) => return Poll::Ready(Err(From::from(err))),
                        Poll::Ready(None) =>
                            return Poll::Ready(Err(NegotiationError::from(
                                ProtocolError::IoError(
                                    io::ErrorKind::UnexpectedEof.into())))),
                        Poll::Pending => {
                            *this.state = State::RecvHeader { io };
                            return Poll::Pending
                        }
                    }
                }

                State::SendHeader { mut io, version } => {
                    match Pin::new(&mut io).poll_ready(cx) {
                        Poll::Pending => {
                            *this.state = State::SendHeader { io, version };
                            return Poll::Pending
                        },
                        Poll::Ready(Ok(())) => {},
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(From::from(err))),
                    }

                    if let Err(err) = Pin::new(&mut io).start_send(Message::Header(version)) {
                        return Poll::Ready(Err(From::from(err)));
                    }

                    *this.state = match version {
                        Version::V1 => State::Flush { io },
                        Version::V1Lazy => State::RecvMessage { io },
                    }
                }

                State::RecvMessage { mut io } => {
                    let msg = match Pin::new(&mut io).poll_next(cx) {
                        Poll::Ready(Some(Ok(msg))) => msg,
                        Poll::Ready(None) =>
                            return Poll::Ready(Err(NegotiationError::from(
                                ProtocolError::IoError(
                                    io::ErrorKind::UnexpectedEof.into())))),
                        Poll::Pending => {
                            *this.state = State::RecvMessage { io };
                            return Poll::Pending;
                        }
                        Poll::Ready(Some(Err(err))) => return Poll::Ready(Err(From::from(err))),
                    };

                    match msg {
                        Message::ListProtocols => {
                            let supported = this.protocols.iter().map(|(_,p)| p).cloned().collect();
                            let message = Message::Protocols(supported);
                            *this.state = State::SendMessage { io, message, protocol: None }
                        }
                        Message::Protocol(p) => {
                            let protocol = this.protocols.iter().find_map(|(name, proto)| {
                                if &p == proto {
                                    Some(name.clone())
                                } else {
                                    None
                                }
                            });

                            let message = if protocol.is_some() {
                                log::debug!("Listener: confirming protocol: {}", p);
                                Message::Protocol(p.clone())
                            } else {
                                log::debug!("Listener: rejecting protocol: {}",
                                    String::from_utf8_lossy(p.as_ref()));
                                Message::NotAvailable
                            };

                            *this.state = State::SendMessage { io, message, protocol };
                        }
                        _ => return Poll::Ready(Err(ProtocolError::InvalidMessage.into()))
                    }
                }

                State::SendMessage { mut io, message, protocol } => {
                    match Pin::new(&mut io).poll_ready(cx) {
                        Poll::Pending => {
                            *this.state = State::SendMessage { io, message, protocol };
                            return Poll::Pending
                        },
                        Poll::Ready(Ok(())) => {},
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(From::from(err))),
                    }

                    if let Err(err) = Pin::new(&mut io).start_send(message) {
                        return Poll::Ready(Err(From::from(err)));
                    }

                    // If a protocol has been selected, finish negotiation.
                    // Otherwise flush the sink and expect to receive another
                    // message.
                    *this.state = match protocol {
                        Some(protocol) => {
                            log::debug!("Listener: sent confirmed protocol: {}",
                                String::from_utf8_lossy(protocol.as_ref()));
                            let (io, remaining) = io.into_inner();
                            let io = Negotiated::completed(io, remaining);
                            return Poll::Ready(Ok((protocol, io)));
                        }
                        None => State::Flush { io }
                    };
                }

                State::Flush { mut io } => {
                    match Pin::new(&mut io).poll_flush(cx) {
                        Poll::Pending => {
                            *this.state = State::Flush { io };
                            return Poll::Pending
                        },
                        Poll::Ready(Ok(())) => *this.state = State::RecvMessage { io },
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(From::from(err))),
                    }
                }

                State::Done => panic!("State::poll called after completion")
            }
        }
    }
}
