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
    I::Item: AsRef<[u8]>,
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
    I::Item: AsRef<[u8]>,
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
                                String::from_utf8_lossy(protocol.as_ref())
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
