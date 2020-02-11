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

use futures::prelude::*;
use crate::protocol::{Protocol, ProtocolError, MessageIO, Message, Version};
use log::{debug, warn};
use smallvec::SmallVec;
use std::{io, iter::FromIterator, mem, convert::TryFrom};
use tokio_io::{AsyncRead, AsyncWrite};
use crate::{Negotiated, NegotiationError};

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
                warn!("Listener: Ignoring invalid protocol: {} due to {}",
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
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]> + Clone
{
    type Item = (N, Negotiated<R>);
    type Error = NegotiationError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::RecvHeader { mut io } => {
                    match io.poll()? {
                        Async::Ready(Some(Message::Header(version))) => {
                            self.state = State::SendHeader { io, version }
                        }
                        Async::Ready(Some(_)) => {
                            return Err(ProtocolError::InvalidMessage.into())
                        }
                        Async::Ready(None) =>
                            return Err(NegotiationError::from(
                                ProtocolError::IoError(
                                    io::ErrorKind::UnexpectedEof.into()))),
                        Async::NotReady => {
                            self.state = State::RecvHeader { io };
                            return Ok(Async::NotReady)
                        }
                    }
                }
                State::SendHeader { mut io, version } => {
                    if io.start_send(Message::Header(version))?.is_not_ready() {
                        return Ok(Async::NotReady)
                    }
                    self.state = match version {
                        Version::V1 => State::Flush { io },
                        Version::V1Lazy => State::RecvMessage { io },
                    }
                }
                State::RecvMessage { mut io } => {
                    let msg = match io.poll() {
                        Ok(Async::Ready(Some(msg))) => msg,
                        Ok(Async::Ready(None)) =>
                            return Err(NegotiationError::from(
                                ProtocolError::IoError(
                                    io::ErrorKind::UnexpectedEof.into()))),
                        Ok(Async::NotReady) => {
                            self.state = State::RecvMessage { io };
                            return Ok(Async::NotReady)
                        }
                        Err(e) => return Err(e.into())
                    };

                    match msg {
                        Message::ListProtocols => {
                            let supported = self.protocols.iter().map(|(_,p)| p).cloned().collect();
                            let message = Message::Protocols(supported);
                            self.state = State::SendMessage { io, message, protocol: None }
                        }
                        Message::Protocol(p) => {
                            let protocol = self.protocols.iter().find_map(|(name, proto)| {
                                if &p == proto {
                                    Some(name.clone())
                                } else {
                                    None
                                }
                            });

                            let message = if protocol.is_some() {
                                debug!("Listener: confirming protocol: {}", p);
                                Message::Protocol(p.clone())
                            } else {
                                debug!("Listener: rejecting protocol: {}",
                                    String::from_utf8_lossy(p.as_ref()));
                                Message::NotAvailable
                            };

                            self.state = State::SendMessage { io, message, protocol };
                        }
                        _ => return Err(ProtocolError::InvalidMessage.into())
                    }
                }
                State::SendMessage { mut io, message, protocol } => {
                    if let AsyncSink::NotReady(message) = io.start_send(message)? {
                        self.state = State::SendMessage { io, message, protocol };
                        return Ok(Async::NotReady)
                    };
                    // If a protocol has been selected, finish negotiation.
                    // Otherwise flush the sink and expect to receive another
                    // message.
                    self.state = match protocol {
                        Some(protocol) => {
                            debug!("Listener: sent confirmed protocol: {}",
                                String::from_utf8_lossy(protocol.as_ref()));
                            let (io, remaining) = io.into_inner();
                            let io = Negotiated::completed(io, remaining);
                            return Ok(Async::Ready((protocol, io)))
                        }
                        None => State::Flush { io }
                    };
                }
                State::Flush { mut io } => {
                    if io.poll_complete()?.is_not_ready() {
                        self.state = State::Flush { io };
                        return Ok(Async::NotReady)
                    }
                    self.state = State::RecvMessage { io }
                }
                State::Done => panic!("State::poll called after completion")
            }
        }
    }
}
