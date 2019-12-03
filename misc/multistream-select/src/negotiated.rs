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

use crate::protocol::{Protocol, Message, Version, ProtocolError};
use futures::{prelude::*, io::{ReadHalf, WriteHalf}};
use log::debug;
use std::{mem, io, error::Error, fmt, pin::Pin, task::Context, task::Poll};

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
#[derive(Debug)]
pub struct Negotiated<TInner> {
    state: State<TInner>
}

impl<TInner> Negotiated<TInner>
where
    TInner: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    /// Creates a `Negotiated` in state [`State::Complete`], possibly
    /// with `remaining` data to be sent.
    pub(crate) fn completed(io: TInner) -> Self {
        let (io_read, io_write) = io.split();
        Negotiated { state: State::Completed { io_read, io_write } }
    }

    /// Creates a `Negotiated` in state [`State::Expecting`] that is still
    /// expecting confirmation of the given `protocol`.
    pub(crate) fn expecting(io: TInner, protocol: Protocol, version: Version) -> Self {
        let (mut io_read, io_write) = io.split();
        let complete_future = Box::pin(async move {
            loop {
                match Message::decode(&mut io_read).await? {
                    Message::Header(v) if v == version => continue,
                    Message::Protocol(p) if p.as_ref() == protocol.as_ref() => {
                        debug!("Negotiated: Received confirmation for protocol: {}", p);
                        break;
                    }
                    _ => return Err(NegotiationError::Failed),
                }
            }

            Ok(io_read)
        });

        Negotiated { state: State::Expecting { complete_future, io_write } }
    }

    /// Returns a `NegotiatedComplete` future that waits for protocol
    /// negotiation to complete.
    pub async fn complete(mut self) -> Result<(), NegotiationError> {
        self.flush().await?;

        match self.state {
            State::Expecting { complete_future, .. } => { complete_future.await?; },
            _ => {}
        }

        Ok(())
    }
}

/// The states of a `Negotiated` I/O stream.
enum State<R> {
    /// In this state, a `Negotiated` is still expecting to
    /// receive confirmation of the protocol it as settled on.
    Expecting {
        /// Future that completes the protocol handshake, then returns the underlying I/O stream.
        complete_future: Pin<Box<dyn Future<Output = Result<ReadHalf<R>, NegotiationError>> + Send>>,
        /// The writing half of underlying I/O stream.
        io_write: WriteHalf<R>,
    },

    /// In this state, a protocol has been agreed upon and may
    /// only be pending the sending of the final acknowledgement,
    /// which is prepended to / combined with the next write for
    /// efficiency.
    // TODO: can be a single `R` after https://github.com/rust-lang/futures-rs/issues/1995
    Completed {
        /// The reading half of underlying I/O stream.
        io_read: ReadHalf<R>,
        /// The writing half of underlying I/O stream.
        io_write: WriteHalf<R>,
    },

    /// Temporary state while moving the `io` resource from
    /// `Expecting` to `Completed`.
    Invalid,
}

impl<TInner> AsyncRead for Negotiated<TInner>
where
    TInner: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8])
        -> Poll<Result<usize, io::Error>>
    {
        loop {
            let io_read = match &mut self.state {
                State::Completed { io_read, .. } => return Pin::new(io_read).poll_read(cx, buf),
                State::Expecting { complete_future, .. } => match Pin::new(complete_future).poll(cx) {
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(From::from(err))),
                    Poll::Ready(Ok(io_read)) => io_read,
                    Poll::Pending => return Poll::Pending,
                },
                State::Invalid => panic!("Negotiated: Invalid state")
            };

            // We only reach here if `complete_future` has completed.
            if let State::Expecting { io_write, .. } = mem::replace(&mut self.state, State::Invalid) {
                self.state = State::Completed { io_read, io_write };
            }
        }
    }
}

impl<TInner> AsyncWrite for Negotiated<TInner>
where
    TInner: AsyncWrite + AsyncRead + Unpin
{
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        match &mut self.state {
            State::Completed { io_write, .. } => Pin::new(io_write).poll_write(cx, buf),
            State::Expecting { io_write, .. } => Pin::new(io_write).poll_write(cx, buf),
            State::Invalid => panic!("Negotiated: Invalid state")
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        match &mut self.state {
            State::Completed { io_write, .. } => Pin::new(io_write).poll_flush(cx),
            State::Expecting { io_write, .. } => Pin::new(io_write).poll_flush(cx),
            State::Invalid => panic!("Negotiated: Invalid state")
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Completing the negotiation might require flushing outgoing data.
        match Pin::new(&mut *self).poll_flush(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(())) => {}
        }

        loop {
            let io_read = match &mut self.state {
                State::Completed { io_write, .. } => return Pin::new(io_write).poll_close(cx),
                State::Expecting { complete_future, .. } => match Pin::new(complete_future).poll(cx) {
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(From::from(err))),
                    Poll::Ready(Ok(io_read)) => io_read,
                    Poll::Pending => return Poll::Pending,
                },
                State::Invalid => panic!("Negotiated: Invalid state")
            };

            // We only reach here if `complete_future` has completed.
            if let State::Expecting { io_write, .. } = mem::replace(&mut self.state, State::Invalid) {
                self.state = State::Completed { io_read, io_write };
            }
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
            return e.into()
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
            NegotiationError::ProtocolError(p) =>
                fmt.write_fmt(format_args!("Protocol error: {}", p)),
            NegotiationError::Failed =>
                fmt.write_str("Protocol negotiation failed.")
        }
    }
}

impl<R> fmt::Debug for State<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            State::Expecting { .. } => f.debug_struct("State::Expecting").finish(),
            State::Completed { .. } => f.debug_struct("State::Completed").finish(),
            State::Invalid => f.debug_struct("State::Invalid").finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;
    use std::io::Write;

    /// An I/O resource with a fixed write capacity (total and per write op).
    struct Capped { buf: Vec<u8>, step: usize }

    impl io::Write for Capped {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.buf.len() + buf.len() > self.buf.capacity() {
                return Err(io::ErrorKind::WriteZero.into())
            }
            self.buf.write(&buf[.. usize::min(self.step, buf.len())])
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl AsyncWrite for Capped {
        fn shutdown(&mut self) -> Poll<(), io::Error> {
            Ok(().into())
        }
    }

    #[test]
    fn write_remaining() {
        fn prop(rem: Vec<u8>, new: Vec<u8>, free: u8) -> TestResult {
            let cap = rem.len() + free as usize;
            let buf = Capped { buf: Vec::with_capacity(cap), step: free as usize };
            let mut rem = BytesMut::from(rem);
            let mut io = Negotiated::completed(buf, rem.clone());
            let mut written = 0;
            loop {
                // Write until `new` has been fully written or the capped buffer is
                // full (in which case the buffer should remain unchanged from the
                // last successful write).
                match io.write(&new[written..]) {
                    Ok(n) =>
                        if let State::Completed { remaining, .. } = &io.state {
                            if n == rem.len() + new[written..].len() {
                                assert!(remaining.is_empty())
                            } else {
                                assert!(remaining.len() <= rem.len());
                            }
                            written += n;
                            if written == new.len() {
                                return TestResult::passed()
                            }
                            rem = remaining.clone();
                        } else {
                            return TestResult::failed()
                        }
                    Err(_) =>
                        if let State::Completed { remaining, .. } = &io.state {
                            assert!(rem.len() + new[written..].len() > cap);
                            assert_eq!(remaining, &rem);
                            return TestResult::passed()
                        } else {
                            return TestResult::failed()
                        }
                }
            }
        }
        quickcheck(prop as fn(_,_,_) -> _)
    }
}

