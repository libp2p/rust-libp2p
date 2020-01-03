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

use bytes::BytesMut;
use crate::protocol::{Protocol, MessageReader, Message, Version, ProtocolError};
use futures::{prelude::*, Async, try_ready};
use log::debug;
use tokio_io::{AsyncRead, AsyncWrite};
use std::{mem, io, fmt, error::Error};

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

/// A `Future` that waits on the completion of protocol negotiation.
#[derive(Debug)]
pub struct NegotiatedComplete<TInner> {
    inner: Option<Negotiated<TInner>>
}

impl<TInner: AsyncRead + AsyncWrite> Future for NegotiatedComplete<TInner> {
    type Item = Negotiated<TInner>;
    type Error = NegotiationError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut io = self.inner.take().expect("NegotiatedFuture called after completion.");
        if io.poll()?.is_not_ready() {
            self.inner = Some(io);
            return Ok(Async::NotReady)
        }
        return Ok(Async::Ready(io))
    }
}

impl<TInner> Negotiated<TInner> {
    /// Creates a `Negotiated` in state [`State::Complete`], possibly
    /// with `remaining` data to be sent.
    pub(crate) fn completed(io: TInner, remaining: BytesMut) -> Self {
        Negotiated { state: State::Completed { io, remaining } }
    }

    /// Creates a `Negotiated` in state [`State::Expecting`] that is still
    /// expecting confirmation of the given `protocol`.
    pub(crate) fn expecting(io: MessageReader<TInner>, protocol: Protocol, version: Version) -> Self {
        Negotiated { state: State::Expecting { io, protocol, version } }
    }

    /// Polls the `Negotiated` for completion.
    fn poll(&mut self) -> Poll<(), NegotiationError>
    where
        TInner: AsyncRead + AsyncWrite
    {
        // Flush any pending negotiation data.
        match self.poll_flush() {
            Ok(Async::Ready(())) => {},
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(e) => {
                // If the remote closed the stream, it is important to still
                // continue reading the data that was sent, if any.
                if e.kind() != io::ErrorKind::WriteZero {
                    return Err(e.into())
                }
            }
        }

        if let State::Completed { remaining, .. } = &mut self.state {
            let _ = remaining.take(); // Drop remaining data flushed above.
            return Ok(Async::Ready(()))
        }

        // Read outstanding protocol negotiation messages.
        loop {
            match mem::replace(&mut self.state, State::Invalid) {
                State::Expecting { mut io, protocol, version } => {
                    let msg = match io.poll() {
                        Ok(Async::Ready(Some(msg))) => msg,
                        Ok(Async::NotReady) => {
                            self.state = State::Expecting { io, protocol, version };
                            return Ok(Async::NotReady)
                        }
                        Ok(Async::Ready(None)) => {
                            self.state = State::Expecting { io, protocol, version };
                            return Err(ProtocolError::IoError(
                                io::ErrorKind::UnexpectedEof.into()).into())
                        }
                        Err(err) => {
                            self.state = State::Expecting { io, protocol, version };
                            return Err(err.into())
                        }
                    };

                    if let Message::Header(v) = &msg {
                        if v == &version {
                            self.state = State::Expecting { io, protocol, version };
                            continue
                        }
                    }

                    if let Message::Protocol(p) = &msg {
                        if p.as_ref() == protocol.as_ref() {
                            debug!("Negotiated: Received confirmation for protocol: {}", p);
                            let (io, remaining) = io.into_inner();
                            self.state = State::Completed { io, remaining };
                            return Ok(Async::Ready(()))
                        }
                    }

                    return Err(NegotiationError::Failed)
                }

                _ => panic!("Negotiated: Invalid state")
            }
        }
    }

    /// Returns a `NegotiatedComplete` future that waits for protocol
    /// negotiation to complete.
    pub fn complete(self) -> NegotiatedComplete<TInner> {
        NegotiatedComplete { inner: Some(self) }
    }
}

/// The states of a `Negotiated` I/O stream.
#[derive(Debug)]
enum State<R> {
    /// In this state, a `Negotiated` is still expecting to
    /// receive confirmation of the protocol it as settled on.
    Expecting {
        /// The underlying I/O stream.
        io: MessageReader<R>,
        /// The expected protocol (i.e. name and version).
        protocol: Protocol,
        /// The expected multistream-select protocol version.
        version: Version
    },

    /// In this state, a protocol has been agreed upon and may
    /// only be pending the sending of the final acknowledgement,
    /// which is prepended to / combined with the next write for
    /// efficiency.
    Completed { io: R, remaining: BytesMut },

    /// Temporary state while moving the `io` resource from
    /// `Expecting` to `Completed`.
    Invalid,
}

impl<R> io::Read for Negotiated<R>
where
    R: AsyncRead + AsyncWrite
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if let State::Completed { io, remaining } = &mut self.state {
                // If protocol negotiation is complete and there is no
                // remaining data to be flushed, commence with reading.
                if remaining.is_empty() {
                    return io.read(buf)
                }
            }

            // Poll the `Negotiated`, driving protocol negotiation to completion,
            // including flushing of any remaining data.
            let result = self.poll();

            // There is still remaining data to be sent before data relating
            // to the negotiated protocol can be read.
            if let Ok(Async::NotReady) = result {
                return Err(io::ErrorKind::WouldBlock.into())
            }

            if let Err(err) = result {
                return Err(err.into())
            }
        }
    }
}

impl<TInner> AsyncRead for Negotiated<TInner>
where
    TInner: AsyncRead + AsyncWrite
{
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match &self.state {
            State::Completed { io, .. } =>
                io.prepare_uninitialized_buffer(buf),
            State::Expecting { io, .. } =>
                io.inner_ref().prepare_uninitialized_buffer(buf),
            State::Invalid => panic!("Negotiated: Invalid state")
        }
    }
}

impl<TInner> io::Write for Negotiated<TInner>
where
    TInner: AsyncWrite
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.state {
            State::Completed { io, ref mut remaining } => {
                while !remaining.is_empty() {
                    let n = io.write(&remaining)?;
                    if n == 0 {
                        return Err(io::ErrorKind::WriteZero.into())
                    }
                    remaining.split_to(n);
                }
                io.write(buf)
            },
            State::Expecting { io, .. } => io.write(buf),
            State::Invalid => panic!("Negotiated: Invalid state")
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.state {
            State::Completed { io, ref mut remaining } => {
                while !remaining.is_empty() {
                    let n = io.write(remaining)?;
                    if n == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "Failed to write remaining buffer."))
                    }
                    remaining.split_to(n);
                }
                io.flush()
            },
            State::Expecting { io, .. } => io.flush(),
            State::Invalid => panic!("Negotiated: Invalid state")
        }
    }
}

impl<TInner> AsyncWrite for Negotiated<TInner>
where
    TInner: AsyncWrite + AsyncRead
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        // Ensure all data has been flushed and expected negotiation messages
        // have been received.
        try_ready!(self.poll().map_err(Into::<io::Error>::into));
        // Continue with the shutdown of the underlying I/O stream.
        match &mut self.state {
            State::Completed { io, .. } => io.shutdown(),
            State::Expecting { io, .. } => io.shutdown(),
            State::Invalid => panic!("Negotiated: Invalid state")
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

impl Into<io::Error> for NegotiationError {
    fn into(self) -> io::Error {
        if let NegotiationError::ProtocolError(e) = self {
            return e.into()
        }
        io::Error::new(io::ErrorKind::Other, self)
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
        fn prop(rem: Vec<u8>, new: Vec<u8>, free: u8, step: u8) -> TestResult {
            let cap = rem.len() + free as usize;
            let step = u8::min(free, step) as usize + 1;
            let buf = Capped { buf: Vec::with_capacity(cap), step };
            let rem = BytesMut::from(rem);
            let mut io = Negotiated::completed(buf, rem.clone());
            let mut written = 0;
            loop {
                // Write until `new` has been fully written or the capped buffer runs
                // over capacity and yields WriteZero.
                match io.write(&new[written..]) {
                    Ok(n) =>
                        if let State::Completed { remaining, .. } = &io.state {
                            assert!(remaining.is_empty());
                            written += n;
                            if written == new.len() {
                                return TestResult::passed()
                            }
                        } else {
                            return TestResult::failed()
                        }
                    Err(e) if e.kind() == io::ErrorKind::WriteZero => {
                        if let State::Completed { .. } = &io.state {
                            assert!(rem.len() + new.len() > cap);
                            return TestResult::passed()
                        } else {
                            return TestResult::failed()
                        }
                    }
                    Err(e) => panic!("Unexpected error: {:?}", e)
                }
            }
        }
        quickcheck(prop as fn(_,_,_,_) -> _)
    }
}

