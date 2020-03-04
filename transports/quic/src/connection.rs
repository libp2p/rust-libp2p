// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

use super::{error::Error, stream_map};
use async_macros::ready;
use either::Either;
use futures::{channel::oneshot, prelude::*};
use libp2p_core::StreamMuxer;
use log::{error, trace};
use parking_lot::{Mutex, MutexGuard};
use std::{
    mem::replace,
    sync::Arc,
    task::{Context, Poll},
};

/// A QUIC substream
#[derive(Debug)]
pub struct Substream {
    id: stream_map::StreamId,
    status: SubstreamStatus,
}

/// The status of a QUIC substream
#[derive(Debug)]
enum SubstreamStatus {
    /// The stream has not been written to yet.  Reading from such a stream will
    /// return an error.
    Unwritten,
    /// The stream is live, and can be read from or written to.
    Live,
    /// The stream is being shut down.  It can be read from, but not written to.
    /// The `oneshot::Receiver` is used to wait for the peer to acknowledge the
    /// shutdown.
    Finishing(oneshot::Receiver<()>),
    /// The stream has been shut down.  Reads are still permissible.  Writes
    /// will return an error.
    Finished,
}

impl Substream {
    /// Construct an unwritten stream. Such a stream must be written to before
    /// it can be read from.
    fn unwritten(id: stream_map::StreamId) -> Self {
        let status = SubstreamStatus::Unwritten;
        Self { id, status }
    }

    /// Construct a live stream, which can be read from or written to.
    fn live(id: stream_map::StreamId) -> Self {
        let status = SubstreamStatus::Live;
        Self { id, status }
    }
}

/// A QUIC connection, either client or server.
///
/// QUIC opens streams lazily, so the peer is not notified that a stream has
/// been opened until data is written (either on this stream, or a
/// higher-numbered one). Therefore, reading on a stream that has not been
/// written to will deadlock, unless another stream is opened and written to
/// before the first read returns. Because this is not needed in practice, and
/// to ease debugging, [`<QuicMuxer as StreamMuxer>::read_substream`] returns an
/// error in this case.
#[derive(Debug, Clone)]
pub struct QuicMuxer(pub(crate) Arc<Mutex<stream_map::Streams>>);

impl QuicMuxer {
    /// Returns the underlying data structure, including all state.
    fn inner(&self) -> MutexGuard<'_, stream_map::Streams> {
        self.0.lock()
    }
}

#[derive(Debug)]
enum OutboundInner {
    /// The substream is fully set up
    Complete(Result<stream_map::StreamId, Error>),
    /// We are waiting on a stream to become available
    Pending(oneshot::Receiver<stream_map::StreamId>),
    /// We have already returned our substream
    Done,
}

/// An outbound QUIC substream. This will eventually resolve to either a
/// [`Substream`] or an [`Error`].
#[derive(Debug)]
pub struct Outbound(OutboundInner);

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = Outbound;
    type Substream = Substream;
    type Error = Error;
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let mut inner = self.inner();
        Outbound(if let Err(e) = inner.close_reason() {
            OutboundInner::Complete(Err(e))
        } else {
            match inner.get_pending_stream() {
                Either::Left(id) => OutboundInner::Complete(Ok(id)),
                Either::Right(receiver) => OutboundInner::Pending(receiver),
            }
        })
    }

    fn destroy_outbound(&self, outbound: Outbound) {
        let mut inner = self.inner();
        let id = match outbound.0 {
            OutboundInner::Complete(Err(_)) => return,
            OutboundInner::Complete(Ok(id)) => id,
            // try_recv is race-free, because the lock on `self` prevents
            // other tasks from sending on the other end of the channel.
            OutboundInner::Pending(mut channel) => match channel.try_recv() {
                Ok(Some(id)) => id,
                Err(oneshot::Canceled) | Ok(None) => return,
            },
            OutboundInner::Done => return,
        };
        inner.destroy_stream(id)
    }

    fn destroy_substream(&self, substream: Self::Substream) {
        self.inner().destroy_stream(substream.id)
    }

    fn is_remote_acknowledged(&self) -> bool {
        // we do not allow 0RTT traffic, for security reasons, so this is
        // always true.
        true
    }

    fn poll_inbound(&self, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        trace!("being polled for inbound connections!");
        let mut inner = self.inner();
        inner.close_reason()?;
        inner.wake_driver();
        let stream = ready!(inner.accept(cx));
        Poll::Ready(Ok(Substream::live(stream)))
    }

    fn write_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::WriteError;
        let mut inner = self.inner();
        inner.wake_driver();
        match inner.write(cx, &substream.id, buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(WriteError::Blocked) => Poll::Pending,
            Err(WriteError::UnknownStream) => {
                error!(
                    "The application used a connection that is already being \
                    closed. This is a bug in the application or in libp2p."
                );
                inner.close_reason()?;
                Poll::Ready(Err(Error::ExpiredStream))
            }
            Err(WriteError::Stopped(e)) => {
                substream.status = SubstreamStatus::Finished;
                Poll::Ready(Err(Error::Stopped(e)))
            }
        }
    }

    fn poll_outbound(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let stream = match substream.0 {
            OutboundInner::Complete(_) => match replace(&mut substream.0, OutboundInner::Done) {
                OutboundInner::Complete(e) => e?,
                _ => unreachable!(),
            },
            OutboundInner::Pending(ref mut receiver) => {
                let result = ready!(receiver.poll_unpin(cx))
                    .map_err(|oneshot::Canceled| Error::ConnectionLost)?;
                substream.0 = OutboundInner::Done;
                result
            }
            OutboundInner::Done => panic!("polled after yielding Ready"),
        };
        Poll::Ready(Ok(Substream::unwritten(stream)))
    }

    /// Try to from a substream. This will return an error if the substream has
    /// not yet been written to.
    fn read_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::ReadError;
        let mut inner = self.inner();
        match inner.read(cx, &substream.id, buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(ReadError::Blocked) => {
                inner.close_reason()?;
                if let SubstreamStatus::Unwritten = substream.status {
                    Poll::Ready(Err(Error::CannotReadFromUnwrittenStream))
                } else {
                    trace!(
                        "Blocked on reading stream {:?} with side {:?}",
                        substream.id,
                        inner.side()
                    );
                    Poll::Pending
                }
            }
            Err(ReadError::UnknownStream) => {
                error!(
                    "The application used a stream that has already been closed. This is a bug."
                );
                Poll::Ready(Err(Error::ExpiredStream))
            }
            Err(ReadError::Reset(e)) => Poll::Ready(Err(Error::Reset(e))),
        }
    }

    fn shutdown_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        match substream.status {
            SubstreamStatus::Finished => return Poll::Ready(Ok(())),
            SubstreamStatus::Finishing(ref mut channel) => {
                self.inner().wake_driver();
                ready!(channel.poll_unpin(cx)).map_err(|_| Error::NetworkFailure)?;
                return Poll::Ready(Ok(()));
            }
            SubstreamStatus::Unwritten | SubstreamStatus::Live => {}
        }
        match self.inner().shutdown_stream(cx, &substream.id) {
            Ok(receiver) => {
                substream.status = SubstreamStatus::Finishing(receiver);
                Poll::Pending
            }
            Err(quinn_proto::FinishError::Stopped(e)) => Poll::Ready(Err(Error::Stopped(e))),
            Err(quinn_proto::FinishError::UnknownStream) => Poll::Ready(Err(Error::ExpiredStream)),
        }
    }

    /// Flush pending data on this stream. libp2p-quic sends out data as soon as
    /// possible, so this method does nothing.
    fn flush_substream(
        &self,
        _cx: &mut Context<'_>,
        _substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    /// Flush all pending data to the peer. libp2p-quic sends out data as soon
    /// as possible, so this method does nothing.
    fn flush_all(&self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    /// Close the connection. Once this function is called, it is a logic error
    /// to call other methods on this object.
    fn close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner().close(cx)
    }
}
