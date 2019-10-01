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

use crate::protocol::{Protocol, ProtocolError, MessageIO, Message, Version};
use futures::{future::Either, prelude::*};
use log::debug;
use std::{io, iter, mem, convert::TryFrom};
use tokio_io::{AsyncRead, AsyncWrite};
use crate::{Negotiated, NegotiationError};

/// Returns a `Future` that negotiates a protocol on the given I/O stream
/// for a peer acting as the _dialer_ (or _initiator_).
///
/// This function is given an I/O stream and a list of protocols and returns a
/// computation that performs the protocol negotiation with the remote. The
/// returned `Future` resolves with the name of the negotiated protocol and
/// a [`Negotiated`] I/O stream.
///
/// The chosen message flow for protocol negotiation depends on the numbers
/// of supported protocols given. That is, this function delegates to
/// [`dialer_select_proto_serial`] or [`dialer_select_proto_parallel`]
/// based on the number of protocols given. The number of protocols is
/// determined through the `size_hint` of the given iterator and thus
/// an inaccurate size estimate may result in a suboptimal choice.
///
/// Within the scope of this library, a dialer always commits to a specific
/// multistream-select protocol [`Version`], whereas a listener always supports
/// all versions supported by this library. Frictionless multistream-select
/// protocol upgrades may thus proceed by deployments with updated listeners,
/// eventually followed by deployments of dialers choosing the newer protocol.
pub fn dialer_select_proto<R, I>(
    inner: R,
    protocols: I,
    version: Version
) -> DialerSelectFuture<R, I::IntoIter>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let iter = protocols.into_iter();
    // We choose between the "serial" and "parallel" strategies based on the number of protocols.
    if iter.size_hint().1.map(|n| n <= 3).unwrap_or(false) {
        Either::A(dialer_select_proto_serial(inner, iter, version))
    } else {
        Either::B(dialer_select_proto_parallel(inner, iter, version))
    }
}

/// Future, returned by `dialer_select_proto`, which selects a protocol and dialer
/// either trying protocols in-order, or by requesting all protocols supported
/// by the remote upfront, from which the first protocol found in the dialer's
/// list of protocols is selected.
pub type DialerSelectFuture<R, I> = Either<DialerSelectSeq<R, I>, DialerSelectPar<R, I>>;

/// Returns a `Future` that negotiates a protocol on the given I/O stream.
///
/// Just like [`dialer_select_proto`] but always using an iterative message flow,
/// trying the given list of supported protocols one-by-one.
///
/// This strategy is preferable if the dialer only supports a few protocols.
pub fn dialer_select_proto_serial<R, I>(
    inner: R,
    protocols: I,
    version: Version
) -> DialerSelectSeq<R, I::IntoIter>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let protocols = protocols.into_iter().peekable();
    DialerSelectSeq {
        version,
        protocols,
        state: SeqState::SendHeader {
            io: MessageIO::new(inner),
        }
    }
}

/// Returns a `Future` that negotiates a protocol on the given I/O stream.
///
/// Just like [`dialer_select_proto`] but always using a message flow that first
/// requests all supported protocols from the remote, selecting the first
/// protocol from the given list of supported protocols that is supported
/// by the remote.
///
/// This strategy may be beneficial if the dialer supports many protocols
/// and it is unclear whether the remote supports one of the first few.
pub fn dialer_select_proto_parallel<R, I>(
    inner: R,
    protocols: I,
    version: Version
) -> DialerSelectPar<R, I::IntoIter>
where
    R: AsyncRead + AsyncWrite,
    I: IntoIterator,
    I::Item: AsRef<[u8]>
{
    let protocols = protocols.into_iter();
    DialerSelectPar {
        version,
        protocols,
        state: ParState::SendHeader {
            io: MessageIO::new(inner)
        }
    }
}

/// A `Future` returned by [`dialer_select_proto_serial`] which negotiates
/// a protocol iteratively by considering one protocol after the other.
pub struct DialerSelectSeq<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    // TODO: It would be nice if eventually N = I::Item = Protocol.
    protocols: iter::Peekable<I>,
    state: SeqState<R, I::Item>,
    version: Version,
}

enum SeqState<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    SendHeader { io: MessageIO<R>, },
    SendProtocol { io: MessageIO<R>, protocol: N },
    FlushProtocol { io: MessageIO<R>, protocol: N },
    AwaitProtocol { io: MessageIO<R>, protocol: N },
    Done
}

impl<R, I> Future for DialerSelectSeq<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    type Item = (I::Item, Negotiated<R>);
    type Error = NegotiationError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, SeqState::Done) {
                SeqState::SendHeader { mut io } => {
                    if io.start_send(Message::Header(self.version))?.is_not_ready() {
                        self.state = SeqState::SendHeader { io };
                        return Ok(Async::NotReady)
                    }
                    let protocol = self.protocols.next().ok_or(NegotiationError::Failed)?;
                    self.state = SeqState::SendProtocol { io, protocol };
                }
                SeqState::SendProtocol { mut io, protocol } => {
                    let p = Protocol::try_from(protocol.as_ref())?;
                    if io.start_send(Message::Protocol(p.clone()))?.is_not_ready() {
                        self.state = SeqState::SendProtocol { io, protocol };
                        return Ok(Async::NotReady)
                    }
                    debug!("Dialer: Proposed protocol: {}", p);
                    if self.protocols.peek().is_some() {
                        self.state = SeqState::FlushProtocol { io, protocol }
                    } else {
                        match self.version {
                            Version::V1 => self.state = SeqState::FlushProtocol { io, protocol },
                            Version::V1Lazy => {
                                debug!("Dialer: Expecting proposed protocol: {}", p);
                                let io = Negotiated::expecting(io.into_reader(), p, self.version);
                                return Ok(Async::Ready((protocol, io)))
                            }
                        }
                    }
                }
                SeqState::FlushProtocol { mut io, protocol } => {
                    if io.poll_complete()?.is_not_ready() {
                        self.state = SeqState::FlushProtocol { io, protocol };
                        return Ok(Async::NotReady)
                    }
                    self.state = SeqState::AwaitProtocol { io, protocol }
                }
                SeqState::AwaitProtocol { mut io, protocol } => {
                    let msg = match io.poll()? {
                        Async::NotReady => {
                            self.state = SeqState::AwaitProtocol { io, protocol };
                            return Ok(Async::NotReady)
                        }
                        Async::Ready(None) =>
                            return Err(NegotiationError::from(
                                io::Error::from(io::ErrorKind::UnexpectedEof))),
                        Async::Ready(Some(msg)) => msg,
                    };

                    match msg {
                        Message::Header(v) if v == self.version => {
                            self.state = SeqState::AwaitProtocol { io, protocol };
                        }
                        Message::Protocol(ref p) if p.as_ref() == protocol.as_ref() => {
                            debug!("Dialer: Received confirmation for protocol: {}", p);
                            let (io, remaining) = io.into_inner();
                            let io = Negotiated::completed(io, remaining);
                            return Ok(Async::Ready((protocol, io)))
                        }
                        Message::NotAvailable => {
                            debug!("Dialer: Received rejection of protocol: {}",
                                String::from_utf8_lossy(protocol.as_ref()));
                            let protocol = self.protocols.next()
                                .ok_or(NegotiationError::Failed)?;
                            self.state = SeqState::SendProtocol { io, protocol }
                        }
                        _ => return Err(ProtocolError::InvalidMessage.into())
                    }
                }
                SeqState::Done => panic!("SeqState::poll called after completion")
            }
        }
    }
}

/// A `Future` returned by [`dialer_select_proto_parallel`] which negotiates
/// a protocol selectively by considering all supported protocols of the remote
/// "in parallel".
pub struct DialerSelectPar<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    protocols: I,
    state: ParState<R, I::Item>,
    version: Version,
}

enum ParState<R, N>
where
    R: AsyncRead + AsyncWrite,
    N: AsRef<[u8]>
{
    SendHeader { io: MessageIO<R> },
    SendProtocolsRequest { io: MessageIO<R> },
    Flush { io: MessageIO<R> },
    RecvProtocols { io: MessageIO<R> },
    SendProtocol { io: MessageIO<R>, protocol: N },
    Done
}

impl<R, I> Future for DialerSelectPar<R, I>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator,
    I::Item: AsRef<[u8]>
{
    type Item = (I::Item, Negotiated<R>);
    type Error = NegotiationError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, ParState::Done) {
                ParState::SendHeader { mut io } => {
                    if io.start_send(Message::Header(self.version))?.is_not_ready() {
                        self.state = ParState::SendHeader { io };
                        return Ok(Async::NotReady)
                    }
                    self.state = ParState::SendProtocolsRequest { io };
                }
                ParState::SendProtocolsRequest { mut io } => {
                    if io.start_send(Message::ListProtocols)?.is_not_ready() {
                        self.state = ParState::SendProtocolsRequest { io };
                        return Ok(Async::NotReady)
                    }
                    debug!("Dialer: Requested supported protocols.");
                    self.state = ParState::Flush { io }
                }
                ParState::Flush { mut io } => {
                    if io.poll_complete()?.is_not_ready() {
                        self.state = ParState::Flush { io };
                        return Ok(Async::NotReady)
                    }
                    self.state = ParState::RecvProtocols { io }
                }
                ParState::RecvProtocols { mut io } => {
                    let msg = match io.poll()? {
                        Async::NotReady => {
                            self.state = ParState::RecvProtocols { io };
                            return Ok(Async::NotReady)
                        }
                        Async::Ready(None) =>
                            return Err(NegotiationError::from(
                                io::Error::from(io::ErrorKind::UnexpectedEof))),
                        Async::Ready(Some(msg)) => msg,
                    };

                    match &msg {
                        Message::Header(v) if v == &self.version => {
                            self.state = ParState::RecvProtocols { io }
                        }
                        Message::Protocols(supported) => {
                            let protocol = self.protocols.by_ref()
                                .find(|p| supported.iter().any(|s|
                                    s.as_ref() == p.as_ref()))
                                .ok_or(NegotiationError::Failed)?;
                            debug!("Dialer: Found supported protocol: {}",
                                String::from_utf8_lossy(protocol.as_ref()));
                            self.state = ParState::SendProtocol { io, protocol };
                        }
                        _ => return Err(ProtocolError::InvalidMessage.into())
                    }
                }
                ParState::SendProtocol { mut io, protocol } => {
                    let p = Protocol::try_from(protocol.as_ref())?;
                    if io.start_send(Message::Protocol(p.clone()))?.is_not_ready() {
                        self.state = ParState::SendProtocol { io, protocol };
                        return Ok(Async::NotReady)
                    }
                    debug!("Dialer: Expecting proposed protocol: {}", p);
                    let io = Negotiated::expecting(io.into_reader(), p, self.version);
                    return Ok(Async::Ready((protocol, io)))
                }
                ParState::Done => panic!("ParState::poll called after completion")
            }
        }
    }
}

