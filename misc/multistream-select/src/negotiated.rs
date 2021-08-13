// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::protocol::{HeaderLine, Message, MessageReader, Protocol, ProtocolError};

use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
    ready,
};
use pin_project::pin_project;
use std::{
    error::Error,
    fmt, io, mem,
    pin::Pin,
    task::{Context, Poll},
};

/// An I/O stream that has settled on an (application-layer) protocol to use.
///
/// A `Negotiated` represents an I/O stream that has _settled_ on a protocol
/// to use. In particular, it is not implied that all of the protocol negotiation
/// frames have yet been sent and / or received, just that the selected protocol
/// is fully determined. This is to allow the last protocol negotiation frames
/// sent by a peer to be combined in a single write, possibly piggy-backing
/// data from the negotiated protocol on top.
///
/// Reading from a `Negotiated` I/O stream that still has pending negotiation
/// protocol data to send implicitly triggers flushing of all yet unsent data.
#[pin_project]
#[derive(Debug)]
pub struct Negotiated<TInner> {
    #[pin]
    state: State<TInner>,
}

/// A `Future` that waits on the completion of protocol negotiation.
#[derive(Debug)]
pub struct NegotiatedComplete<TInner> {
    inner: Option<Negotiated<TInner>>,
}

impl<TInner> Future for NegotiatedComplete<TInner>
where
    // `Unpin` is required not because of implementation details but because we produce the
    // `Negotiated` as the output of the future.
    TInner: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Result<Negotiated<TInner>, NegotiationError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut io = self
            .inner
            .take()
            .expect("NegotiatedFuture called after completion.");
        match Negotiated::poll(Pin::new(&mut io), cx) {
            Poll::Pending => {
                self.inner = Some(io);
                Poll::Pending
            }
            Poll::Ready(Ok(())) => Poll::Ready(Ok(io)),
            Poll::Ready(Err(err)) => {
                self.inner = Some(io);
                Poll::Ready(Err(err))
            }
        }
    }
}

impl<TInner> Negotiated<TInner> {
    /// Creates a `Negotiated` in state [`State::Completed`].
    pub(crate) fn completed(io: TInner) -> Self {
        Negotiated {
            state: State::Completed { io },
        }
    }

    /// Creates a `Negotiated` in state [`State::Expecting`] that is still
    /// expecting confirmation of the given `protocol`.
    pub(crate) fn expecting(
        io: MessageReader<TInner>,
        protocol: Protocol,
        header: Option<HeaderLine>,
    ) -> Self {
        Negotiated {
            state: State::Expecting {
                io,
                protocol,
                header,
            },
        }
    }

    /// Polls the `Negotiated` for completion.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), NegotiationError>>
    where
        TInner: AsyncRead + AsyncWrite + Unpin,
    {
        // Flush any pending negotiation data.
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => {
                // If the remote closed the stream, it is important to still
                // continue reading the data that was sent, if any.
                if e.kind() != io::ErrorKind::WriteZero {
                    return Poll::Ready(Err(e.into()));
                }
            }
        }

        let mut this = self.project();

        if let StateProj::Completed { .. } = this.state.as_mut().project() {
            return Poll::Ready(Ok(()));
        }

        // Read outstanding protocol negotiation messages.
        loop {
            match mem::replace(&mut *this.state, State::Invalid) {
                State::Expecting {
                    mut io,
                    header,
                    protocol,
                } => {
                    let msg = match Pin::new(&mut io).poll_next(cx)? {
                        Poll::Ready(Some(msg)) => msg,
                        Poll::Pending => {
                            *this.state = State::Expecting {
                                io,
                                header,
                                protocol,
                            };
                            return Poll::Pending;
                        }
                        Poll::Ready(None) => {
                            return Poll::Ready(Err(ProtocolError::IoError(
                                io::ErrorKind::UnexpectedEof.into(),
                            )
                            .into()));
                        }
                    };

                    if let Message::Header(h) = &msg {
                        if Some(h) == header.as_ref() {
                            *this.state = State::Expecting {
                                io,
                                protocol,
                                header: None,
                            };
                            continue;
                        }
                    }

                    if let Message::Protocol(p) = &msg {
                        if p.as_ref() == protocol.as_ref() {
                            log::debug!("Negotiated: Received confirmation for protocol: {}", p);
                            *this.state = State::Completed {
                                io: io.into_inner(),
                            };
                            return Poll::Ready(Ok(()));
                        }
                    }

                    return Poll::Ready(Err(NegotiationError::Failed));
                }

                _ => panic!("Negotiated: Invalid state"),
            }
        }
    }

    /// Returns a [`NegotiatedComplete`] future that waits for protocol
    /// negotiation to complete.
    pub fn complete(self) -> NegotiatedComplete<TInner> {
        NegotiatedComplete { inner: Some(self) }
    }
}

/// The states of a `Negotiated` I/O stream.
#[pin_project(project = StateProj)]
#[derive(Debug)]
enum State<R> {
    /// In this state, a `Negotiated` is still expecting to
    /// receive confirmation of the protocol it has optimistically
    /// settled on.
    Expecting {
        /// The underlying I/O stream.
        #[pin]
        io: MessageReader<R>,
        /// The expected negotiation header/preamble (i.e. multistream-select version),
        /// if one is still expected to be received.
        header: Option<HeaderLine>,
        /// The expected application protocol (i.e. name and version).
        protocol: Protocol,
    },

    /// In this state, a protocol has been agreed upon and I/O
    /// on the underlying stream can commence.
    Completed {
        #[pin]
        io: R,
    },

    /// Temporary state while moving the `io` resource from
    /// `Expecting` to `Completed`.
    Invalid,
}

impl<TInner> AsyncRead for Negotiated<TInner>
where
    TInner: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        loop {
            if let StateProj::Completed { io } = self.as_mut().project().state.project() {
                // If protocol negotiation is complete, commence with reading.
                return io.poll_read(cx, buf);
            }

            // Poll the `Negotiated`, driving protocol negotiation to completion,
            // including flushing of any remaining data.
            match self.as_mut().poll(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(From::from(err))),
            }
        }
    }

    // TODO: implement once method is stabilized in the futures crate
    /*unsafe fn initializer(&self) -> Initializer {
        match &self.state {
            State::Completed { io, .. } => io.initializer(),
            State::Expecting { io, .. } => io.inner_ref().initializer(),
            State::Invalid => panic!("Negotiated: Invalid state"),
        }
    }*/

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        loop {
            if let StateProj::Completed { io } = self.as_mut().project().state.project() {
                // If protocol negotiation is complete, commence with reading.
                return io.poll_read_vectored(cx, bufs);
            }

            // Poll the `Negotiated`, driving protocol negotiation to completion,
            // including flushing of any remaining data.
            match self.as_mut().poll(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(From::from(err))),
            }
        }
    }
}

impl<TInner> AsyncWrite for Negotiated<TInner>
where
    TInner: AsyncWrite + AsyncRead + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        match self.project().state.project() {
            StateProj::Completed { io } => io.poll_write(cx, buf),
            StateProj::Expecting { io, .. } => io.poll_write(cx, buf),
            StateProj::Invalid => panic!("Negotiated: Invalid state"),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        match self.project().state.project() {
            StateProj::Completed { io } => io.poll_flush(cx),
            StateProj::Expecting { io, .. } => io.poll_flush(cx),
            StateProj::Invalid => panic!("Negotiated: Invalid state"),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        // Ensure all data has been flushed and expected negotiation messages
        // have been received.
        ready!(self.as_mut().poll(cx).map_err(Into::<io::Error>::into)?);
        ready!(self
            .as_mut()
            .poll_flush(cx)
            .map_err(Into::<io::Error>::into)?);

        // Continue with the shutdown of the underlying I/O stream.
        match self.project().state.project() {
            StateProj::Completed { io, .. } => io.poll_close(cx),
            StateProj::Expecting { io, .. } => io.poll_close(cx),
            StateProj::Invalid => panic!("Negotiated: Invalid state"),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        match self.project().state.project() {
            StateProj::Completed { io } => io.poll_write_vectored(cx, bufs),
            StateProj::Expecting { io, .. } => io.poll_write_vectored(cx, bufs),
            StateProj::Invalid => panic!("Negotiated: Invalid state"),
        }
    }
}

/// Error that can happen when negotiating a protocol with the remote.
#[derive(Debug)]
pub enum NegotiationError {
    /// A protocol error occurred during the negotiation.
    ProtocolError(ProtocolError),

    /// Protocol negotiation failed because no protocol could be agreed upon.
    Failed,
}

impl From<ProtocolError> for NegotiationError {
    fn from(err: ProtocolError) -> NegotiationError {
        NegotiationError::ProtocolError(err)
    }
}

impl From<io::Error> for NegotiationError {
    fn from(err: io::Error) -> NegotiationError {
        ProtocolError::from(err).into()
    }
}

impl From<NegotiationError> for io::Error {
    fn from(err: NegotiationError) -> io::Error {
        if let NegotiationError::ProtocolError(e) = err {
            return e.into();
        }
        io::Error::new(io::ErrorKind::Other, err)
    }
}

impl Error for NegotiationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            NegotiationError::ProtocolError(err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for NegotiationError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            NegotiationError::ProtocolError(p) => {
                fmt.write_fmt(format_args!("Protocol error: {}", p))
            }
            NegotiationError::Failed => fmt.write_str("Protocol negotiation failed."),
        }
    }
}
