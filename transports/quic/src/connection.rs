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

use super::{endpoint::ConnectionEndpoint as Endpoint, error::Error, socket};
use async_macros::ready;
use futures::{channel::oneshot, prelude::*};
use libp2p_core::StreamMuxer;
use log::{debug, error, info, trace, warn};
use parking_lot::{Mutex, MutexGuard};
use quinn_proto::Dir;
use std::{
    mem::replace,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};

mod stream;
mod stream_map;

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
pub struct QuicMuxer(Arc<Mutex<Muxer>>);

impl QuicMuxer {
    /// Returns the underlying data structure, including all state.
    fn inner(&self) -> MutexGuard<'_, Muxer> {
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
        if let Some(ref e) = inner.close_reason {
            Outbound(OutboundInner::Complete(Err(Error::ConnectionError(
                e.clone(),
            ))))
        } else if let Some(id) = inner.get_pending_stream() {
            Outbound(OutboundInner::Complete(Ok(id)))
        } else {
            let (sender, receiver) = oneshot::channel();
            inner.connectors.push_front(sender);
            inner.wake_driver();
            Outbound(OutboundInner::Pending(receiver))
        }
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
        inner.connection.destroy_stream(*id);
        inner.streams.remove(id)
    }

    fn destroy_substream(&self, substream: Self::Substream) {
        let mut inner = self.inner();
        inner.connection.destroy_stream(*substream.id);
        trace!("Removing substream {:?} from map", substream.id);
        inner.streams.remove(substream.id);
    }

    fn is_remote_acknowledged(&self) -> bool {
        // we do not allow 0RTT traffic, for security reasons, so this is
        // always true.
        true
    }

    fn poll_inbound(&self, cx: &mut Context<'_>) -> Poll<Result<Self::Substream, Self::Error>> {
        trace!("being polled for inbound connections!");
        let mut inner = self.inner();
        if let Some(close_reason) = &inner.close_reason {
            return Poll::Ready(Err(Error::ConnectionError(close_reason.clone())));
        }
        inner.wake_driver();
        match inner.connection.accept() {
            None => {
                inner.handshake_or_accept_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Some(id) => {
                inner.outbound_streams += 1;
                Poll::Ready(Ok(Substream::live(inner.streams.add_stream(id))))
            }
        }
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
        if let Some(ref e) = inner.close_reason {
            return Poll::Ready(Err(Error::ConnectionError(e.clone())));
        }
        match inner.connection.write(*substream.id, buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(WriteError::Blocked) => {
                if let Some(ref e) = inner.close_reason {
                    return Poll::Ready(Err(Error::ConnectionError(e.clone())));
                }
                inner.streams.set_writer(&substream.id, cx.waker().clone());
                Poll::Pending
            }
            Err(WriteError::UnknownStream) => {
                error!(
                    "The application used a connection that is already being \
                    closed. This is a bug in the application or in libp2p."
                );
                if let Some(e) = &inner.close_reason {
                    Poll::Ready(Err(Error::ConnectionError(e.clone())))
                } else {
                    Poll::Ready(Err(Error::ExpiredStream))
                }
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
        inner.wake_driver();
        match inner.connection.read(*substream.id, buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(ReadError::Blocked) => {
                if let Some(error) = &inner.close_reason {
                    Poll::Ready(Err(Error::ConnectionError(error.clone())))
                } else if let SubstreamStatus::Unwritten = substream.status {
                    Poll::Ready(Err(Error::CannotReadFromUnwrittenStream))
                } else {
                    trace!(
                        "Blocked on reading stream {:?} with side {:?}",
                        substream.id,
                        inner.connection.side()
                    );
                    inner.streams.set_reader(&substream.id, cx.waker().clone());
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
                match channel.poll_unpin(cx) {
                    Poll::Ready(Ok(())) | Poll::Ready(Err(oneshot::Canceled)) => {
                        return Poll::Ready(Ok(()))
                    }
                    Poll::Pending => {}
                }
            }
            SubstreamStatus::Unwritten | SubstreamStatus::Live => {}
        }
        let mut inner = self.inner();
        inner.wake_driver();
        inner
            .connection
            .finish(*substream.id)
            .map_err(|e| match e {
                quinn_proto::FinishError::UnknownStream => Error::ConnectionClosing,
                quinn_proto::FinishError::Stopped(e) => Error::Stopped(e),
            })?;
        let (sender, mut receiver) = oneshot::channel();
        assert!(
            receiver.poll_unpin(cx).is_pending(),
            "we haven’t written to the peer yet"
        );
        substream.status = SubstreamStatus::Finishing(receiver);
        inner.streams.set_finisher(&substream.id, sender);
        Poll::Pending
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
        let mut inner = self.inner();
        trace!(
            "close() called with {} outbound streams for side {:?}",
            inner.outbound_streams,
            inner.connection.side()
        );
        inner.wake_closer();
        inner.close_waker = Some(cx.waker().clone());
        if inner.close_reason.is_some() {
            return Poll::Ready(Ok(()));
        } else if inner.outbound_streams == 0 {
            inner.shutdown(0);
            inner.close_reason = Some(quinn_proto::ConnectionError::LocallyClosed);
            return Poll::Ready(Ok(()));
        }
        let Muxer {
            streams,
            connection,
            ..
        } = &mut *inner;
        streams.close(|id| drop(connection.finish(id)));
        Poll::Pending
    }
}

/// A QUIC connection that is
#[derive(Debug)]
pub struct Upgrade {
    muxer: Option<QuicMuxer>,
}

#[cfg(test)]
impl Drop for Upgrade {
    fn drop(&mut self) {
        debug!("dropping upgrade!");
        assert!(
            self.muxer.is_none(),
            "dropped before being polled to completion"
        );
    }
}

impl Future for Upgrade {
    type Output = Result<(libp2p_core::PeerId, QuicMuxer), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let muxer = &mut self.get_mut().muxer;
        trace!("outbound polling!");
        let mut inner = muxer.as_mut().expect("polled after yielding Ready").inner();
        if let Some(close_reason) = &inner.close_reason {
            return Poll::Ready(Err(Error::ConnectionError(close_reason.clone())));
        } else if inner.connection.is_handshaking() {
            inner.handshake_or_accept_waker = Some(cx.waker().clone());
            return Poll::Pending;
        } else {
            match ready!(inner.connection.notify_handshake_complete(cx)) {
                Ok(res) => {
                    drop(inner);
                    Poll::Ready(Ok((
                        res,
                        muxer.take().expect("polled after yielding Ready"),
                    )))
                }
                Err(e) => {
                    inner.shutdown(RESET);
                    inner.close_reason = Some(quinn_proto::ConnectionError::LocallyClosed);
                    drop(inner);
                    *muxer = None;
                    Poll::Ready(Err(e))
                }
            }
        }
    }
}

type StreamSenderQueue = std::collections::VecDeque<oneshot::Sender<stream_map::StreamId>>;

/// A QUIC connection and its associated data.
#[derive(Debug)]
pub(crate) struct Muxer {
    /// If this is `Some`, it is a stream that has been opened by the peer,
    /// but not yet accepted by the application.  Otherwise, this is `None`.
    pending_stream: Option<stream_map::StreamId>,
    /// The QUIC state machine and endpoint messaging.
    connection: Endpoint,
    /// The stream statuses
    streams: stream_map::Streams,
    /// Task waiting for new connections, or for the connection to finish handshaking.
    handshake_or_accept_waker: Option<std::task::Waker>,
    /// Tasks waiting to make a connection.
    connectors: StreamSenderQueue,
    /// The number of outbound streams currently open.
    outbound_streams: usize,
    /// The close reason, if this connection has been lost
    close_reason: Option<quinn_proto::ConnectionError>,
    /// Waker to wake up the driver. This is not a `MaybeWaker` because only the
    /// driver task ever puts a waker here.
    waker: Option<std::task::Waker>,
    /// Close waker
    close_waker: Option<std::task::Waker>,
}

const RESET: u32 = 1;

impl Drop for Muxer {
    fn drop(&mut self) {
        if self.close_reason.is_none() {
            warn!("connection uncleanly closed!");
            self.shutdown(RESET)
        }
    }
}

impl Muxer {
    pub(crate) fn wake_driver(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }

    fn wake_closer(&mut self) {
        if let Some(close_waker) = self.close_waker.take() {
            close_waker.wake()
        }
    }

    pub(crate) fn connection(&mut self) -> &mut Endpoint {
        &mut self.connection
    }

    fn wake_incoming(&mut self) {
        if let Some(waker) = self.handshake_or_accept_waker.take() {
            waker.wake()
        }
    }

    fn new(connection: Endpoint) -> Self {
        Muxer {
            close_waker: None,
            outbound_streams: 0,
            pending_stream: None,
            streams: Default::default(),
            handshake_or_accept_waker: None,
            connectors: Default::default(),
            connection,
            close_reason: None,
            waker: None,
        }
    }
    fn shutdown(&mut self, error_code: u32) {
        debug!("shutting connection down!");
        self.wake_incoming();
        self.streams.wake_all();
        self.connectors.clear();
        self.connection.close(error_code);
        self.wake_driver();
    }

    fn open_stream(&mut self) -> Option<stream_map::StreamId> {
        self.connection.open().map(|e| {
            self.outbound_streams += 1;
            self.streams.add_stream(e)
        })
    }

    fn get_pending_stream(&mut self) -> Option<stream_map::StreamId> {
        self.wake_driver();
        if let Some(id) = self.pending_stream.take() {
            Some(id)
        } else {
            self.open_stream()
        }
    }

    pub(crate) fn process_app_events(&mut self) {
        use quinn_proto::Event;
        'a: while let Some(event) = self.connection.poll() {
            match event {
                Event::StreamOpened { dir: Dir::Uni } | Event::DatagramReceived => {
                    // This should never happen, but if it does, it is better to
                    // log a nasty message and recover than to tear down the
                    // process.
                    error!("we disabled incoming unidirectional streams and datagrams")
                }
                Event::StreamAvailable { dir: Dir::Uni } => {
                    // Ditto
                    error!("we don’t use unidirectional streams")
                }
                Event::StreamReadable { stream } => {
                    trace!(
                        "Stream {:?} readable for side {:?}",
                        stream,
                        self.connection.side()
                    );
                    // Wake up the task waiting on us (if any)
                    self.streams.wake_reader(stream);
                }
                Event::StreamWritable { stream } => {
                    trace!(
                        "Stream {:?} writable for side {:?}",
                        stream,
                        self.connection.side()
                    );
                    // Wake up the task waiting on us (if any).
                    // This will panic if quinn-proto has already emitted a
                    // `StreamFinished` event, but it will never emit
                    // `StreamWritable` after `StreamFinished`.
                    self.streams.wake_writer(stream)
                }
                Event::StreamAvailable { dir: Dir::Bi } => {
                    trace!(
                        "Bidirectional stream available for side {:?}",
                        self.connection.side()
                    );
                    if self.connectors.is_empty() {
                        // Nobody is waiting on the stream. Allow quinn-proto to
                        // queue it, so that it can apply backpressure.
                        continue;
                    }
                    assert!(
                        self.pending_stream.is_none(),
                        "we cannot have both pending tasks and a pending stream; qed"
                    );
                    let mut stream = self.open_stream().expect(
                        "we just were told that there is a stream available; there is \
                        a mutex that prevents other threads from calling open() in the \
                        meantime; qed",
                    );
                    while let Some(oneshot) = self.connectors.pop_front() {
                        stream = match oneshot.send(stream) {
                            Ok(()) => continue 'a,
                            Err(e) => e,
                        }
                    }
                    self.pending_stream = Some(stream)
                }
                Event::ConnectionLost { reason } => {
                    debug!(
                        "lost connection due to {:?} for side {:?}",
                        reason,
                        self.connection.side()
                    );
                    self.close_reason = Some(reason);
                    self.wake_closer();
                    self.shutdown(0);
                }
                Event::StreamFinished {
                    stream,
                    stop_reason,
                } => {
                    trace!(
                        "Stream {:?} finished for side {:?} because of {:?}",
                        stream,
                        self.connection.side(),
                        stop_reason
                    );
                    // This stream is no longer useable for outbound traffic.
                    self.outbound_streams -= 1;
                    // If someone is waiting for this stream to become writable,
                    // wake them up, so that they can find out the stream is
                    // dead.
                    self.streams.wake_writer(stream);
                    // Connection close could be blocked on this.
                    self.wake_closer()
                }
                Event::Connected => {
                    debug!("connected for side {:?}!", self.connection.side());
                    self.wake_incoming();
                }
                Event::StreamOpened { dir: Dir::Bi } => {
                    debug!("stream opened for side {:?}", self.connection.side());
                    self.wake_incoming()
                }
            }
        }
    }
}

/// The connection driver
#[derive(Debug)]
pub(super) struct ConnectionDriver {
    inner: Arc<Mutex<Muxer>>,
    /// The packet awaiting transmission, if any.
    outgoing_packet: Option<quinn_proto::Transmit>,
    /// The timer being used by this connection
    timer: Option<futures_timer::Delay>,
    /// The last timeout returned by `quinn_proto::poll_timeout`.
    last_timeout: Option<Instant>,
    /// Transmit socket
    socket: Arc<socket::Socket>,
}

impl ConnectionDriver {
    pub(crate) fn spawn<T: FnOnce(Arc<Mutex<Muxer>>) -> Arc<crate::socket::Socket>>(
        connection: Endpoint,
        cb: T,
    ) -> Upgrade {
        let inner = Arc::new(Mutex::new(Muxer::new(connection)));
        let socket = cb(inner.clone());
        async_std::task::spawn(Self {
            inner: inner.clone(),
            outgoing_packet: None,
            timer: None,
            last_timeout: None,
            socket,
        });
        Upgrade {
            muxer: Some(QuicMuxer(inner)),
        }
    }
}

impl Future for ConnectionDriver {
    type Output = Result<(), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        debug!("being polled for timer!");
        let mut inner = this.inner.lock();
        inner.waker = Some(cx.waker().clone());
        loop {
            let now = Instant::now();
            inner
                .connection
                .poll_transmit_pending(now, cx, &this.socket)?;
            inner.process_app_events();
            inner.connection.send_endpoint_events(cx)?;
            match inner.connection.poll_timeout() {
                None => {
                    this.timer = None;
                    this.last_timeout = None
                }
                Some(t) if t <= now => {
                    inner.connection.handle_timeout(now);
                    continue;
                }
                t if t == this.last_timeout => {}
                Some(t) => this.timer = Some(futures_timer::Delay::new(t - now)),
            }
            if let Some(ref mut timer) = this.timer {
                if timer.poll_unpin(cx).is_ready() {
                    inner.connection.handle_timeout(now);
                    continue;
                }
            }
            if !inner.connection.is_drained() {
                break Poll::Pending;
            }
            info!("exiting driver");
            let close_reason = inner
                .close_reason
                .clone()
                .expect("we never have a closed connection with no reason; qed");
            break Poll::Ready(match close_reason {
                quinn_proto::ConnectionError::LocallyClosed => Ok(()),
                e => Err(e.into()),
            });
        }
    }
}
