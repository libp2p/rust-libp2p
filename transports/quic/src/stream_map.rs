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

//! The state of all active streams in a QUIC connection

use super::endpoint::ConnectionEndpoint;
use super::stream::StreamState;
use super::{socket, Error};
use async_macros::ready;
use either::Either;
use futures::{channel::oneshot, future::FutureExt};
use log::{debug, error, info, trace};
use parking_lot::Mutex;
use quinn_proto::Dir;
use std::collections::HashMap;
use std::sync::Arc;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Instant,
};

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
/// A stream ID.
pub(super) struct StreamId(quinn_proto::StreamId);

impl std::ops::Deref for StreamId {
    type Target = quinn_proto::StreamId;
    fn deref(&self) -> &quinn_proto::StreamId {
        &self.0
    }
}

/// A QUIC connection that is
#[derive(Debug)]
pub struct Upgrade {
    muxer: Option<Arc<Mutex<Streams>>>,
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

type StreamSenderQueue = std::collections::VecDeque<oneshot::Sender<StreamId>>;

/// A set of streams.
#[derive(Debug)]
pub(super) struct Streams {
    /// If this is `Some`, it is a stream that has been opened by the peer,
    /// but not yet accepted by the application.  Otherwise, this is `None`.
    pending_stream: Option<StreamId>,
    /// The stream statuses
    map: HashMap<quinn_proto::StreamId, StreamState>,
    /// The QUIC state machine and endpoint messaging.
    connection: ConnectionEndpoint,
    /// The number of outbound streams currently open.
    outbound_streams: usize,
    /// Tasks waiting to make a connection.
    connectors: StreamSenderQueue,
    /// The close reason, if this connection has been lost
    close_reason: Option<quinn_proto::ConnectionError>,
    /// Waker to wake up the driver. This is not a `MaybeWaker` because only the
    /// driver task ever puts a waker here.
    waker: Option<Waker>,
    /// Close waker
    close_waker: Option<Waker>,
}

impl Streams {
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

    fn add_stream(&mut self, id: quinn_proto::StreamId) -> StreamId {
        if self.map.insert(id, Default::default()).is_some() {
            panic!(
                "Internal state corrupted. \
                You probably used a Substream with the wrong StreamMuxer",
            )
        }
        StreamId(id)
    }

    fn new(connection: ConnectionEndpoint) -> Self {
        Self {
            pending_stream: None,
            map: Default::default(),
            connection,
            outbound_streams: 0,
            connectors: Default::default(),
            close_reason: None,
            waker: None,
            close_waker: None,
        }
    }

    fn get(&mut self, id: &StreamId) -> &mut StreamState {
        self.map.get_mut(id).expect(
            "Internal state corrupted. \
            You probably used a Substream with the wrong StreamMuxer",
        )
    }

    /// Indicate that the stream is open for reading. Calling this when nobody
    /// is waiting for this stream to be readable is a harmless no-op.
    pub(super) fn wake_reader(&mut self, id: quinn_proto::StreamId) {
        if let Some(stream) = self.map.get_mut(&id) {
            stream.wake_reader()
        }
    }

    /// If a task is waiting for this stream to be finished or written to, wake
    /// it up. Otherwise, do nothing.
    pub(super) fn wake_writer(&mut self, id: quinn_proto::StreamId) {
        if let Some(stream) = self.map.get_mut(&id) {
            stream.wake_writer()
        }
    }

    /// Remove an ID from the map.
    ///
    /// # Panics
    ///
    /// Panics if the ID has already been removed.
    pub(super) fn remove(&mut self, id: StreamId) {
        self.map.remove(&id.0).expect(
            "Internal state corrupted. \
             You probably used a Substream with the wrong StreamMuxer",
        );
    }

    /// Poll for incoming streams
    pub(super) fn accept(&mut self, cx: &mut Context<'_>) -> Poll<StreamId> {
        self.wake_driver();
        self.connection.accept(cx).map(|e| {
            self.outbound_streams += 1;
            self.add_stream(e)
        })
    }

    /// Wake up everything
    fn wake_all(&mut self) {
        for value in self.map.values_mut() {
            value.wake_all()
        }
    }

    pub(super) fn process_app_events(&mut self) {
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
                    self.wake_reader(stream);
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
                    self.wake_writer(stream)
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
                    self.wake_writer(stream);
                    // Connection close could be blocked on this.
                    self.wake_closer()
                }
                Event::Connected => {
                    debug!("connected for side {:?}!", self.connection.side());
                    self.connection.wake();
                }
                Event::StreamOpened { dir: Dir::Bi } => {
                    debug!("stream opened for side {:?}", self.connection.side());
                    self.connection.wake()
                }
            }
        }
    }

    fn shutdown(&mut self, error_code: u32) {
        debug!("shutting connection down!");
        self.connection.wake();
        self.wake_all();
        self.connectors.clear();
        self.connection.close(error_code);
        self.wake_driver();
    }

    fn open_stream(&mut self) -> Option<StreamId> {
        self.connection.open().map(|e| {
            self.outbound_streams += 1;
            self.add_stream(e)
        })
    }

    pub(crate) fn write(
        &mut self,
        cx: &mut Context<'_>,
        substream: &StreamId,
        buf: &[u8],
    ) -> Result<usize, quinn_proto::WriteError> {
        use quinn_proto::WriteError;
        self.wake_driver();
        match self.connection.write(substream.0, buf) {
            e @ Err(WriteError::Blocked) => {
                self.get(substream).set_writer(cx.waker().clone());
                e
            }
            e => e,
        }
    }

    pub(crate) fn side(&self) -> quinn_proto::Side {
        self.connection.side()
    }

    pub(crate) fn read(
        &mut self,
        cx: &mut Context<'_>,
        substream: &StreamId,
        buf: &mut [u8],
    ) -> Result<usize, quinn_proto::ReadError> {
        use quinn_proto::ReadError;
        self.wake_driver();
        match self.connection.read(substream.0, buf) {
            e @ Err(ReadError::Blocked) => {
                self.get(substream).set_reader(cx.waker().clone());
                e
            }
            e => e,
        }
    }

    pub(crate) fn get_pending_stream(&mut self) -> Either<StreamId, oneshot::Receiver<StreamId>> {
        self.wake_driver();
        let pending = std::mem::replace(&mut self.pending_stream, None);
        match pending.or_else(|| self.open_stream()) {
            Some(stream) => Either::Left(stream),
            None => {
                let (sender, receiver) = oneshot::channel();
                self.connectors.push_front(sender);
                self.wake_driver();
                Either::Right(receiver)
            }
        }
    }

    pub(crate) fn close_reason(&self) -> Result<(), Error> {
        match self
            .close_reason
            .as_ref()
            .map(|e| Error::ConnectionError(e.clone()))
        {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub(crate) fn destroy_stream(&mut self, stream: StreamId) {
        trace!("Removing substream {:?} from map", *stream);
        self.connection.destroy_stream(*stream);
        self.remove(stream)
    }

    pub(crate) fn close(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        trace!(
            "close() called with {} outbound streams for side {:?}",
            self.outbound_streams,
            self.connection.side()
        );
        self.wake_closer();
        self.close_waker = Some(cx.waker().clone());
        if self.close_reason.is_some() {
            return Poll::Ready(Ok(()));
        } else if self.outbound_streams == 0 {
            self.shutdown(0);
            self.close_reason = Some(quinn_proto::ConnectionError::LocallyClosed);
            return Poll::Ready(Ok(()));
        }
        for (&stream, value) in &mut self.map {
            value.wake_all();
            let _ = self.connection.finish(stream);
        }
        Poll::Pending
    }

    pub(crate) fn handle_event(
        &mut self,
        event: quinn_proto::ConnectionEvent,
    ) -> Option<quinn_proto::EndpointEvent> {
        self.connection.handle_event(event)
    }

    pub(crate) fn shutdown_stream(
        &mut self,
        cx: &mut Context<'_>,
        substream: &StreamId,
    ) -> Result<oneshot::Receiver<()>, quinn_proto::FinishError> {
        self.wake_driver();
        self.connection.finish(**substream)?;
        let (sender, mut receiver) = oneshot::channel();
        let _ = receiver.poll_unpin(cx);
        self.get(substream).set_finisher(sender);
        Ok(receiver)
    }
}

/// The connection driver
#[derive(Debug)]
struct ConnectionDriver {
    inner: Arc<Mutex<Streams>>,
    /// The packet awaiting transmission, if any.
    outgoing_packet: Option<quinn_proto::Transmit>,
    /// The timer being used by this connection
    timer: Option<futures_timer::Delay>,
    /// The last timeout returned by `quinn_proto::poll_timeout`.
    last_timeout: Option<Instant>,
    /// Transmit socket
    socket: Arc<socket::Socket>,
}

impl Upgrade {
    pub(crate) fn spawn<T: FnOnce(Arc<Mutex<Streams>>) -> Arc<crate::socket::Socket>>(
        connection: ConnectionEndpoint,
        cb: T,
    ) -> Upgrade {
        let inner = Arc::new(Mutex::new(Streams::new(connection)));
        let socket = cb(inner.clone());
        async_std::task::spawn(ConnectionDriver {
            inner: inner.clone(),
            outgoing_packet: None,
            timer: None,
            last_timeout: None,
            socket,
        });
        Upgrade { muxer: Some(inner) }
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

const RESET: u32 = 1;

impl Future for Upgrade {
    type Output = Result<(libp2p_core::PeerId, crate::Muxer), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let muxer = &mut self.get_mut().muxer;
        trace!("outbound polling!");
        let mut inner = muxer.as_mut().expect("polled after yielding Ready").lock();
        if let Some(close_reason) = &inner.close_reason {
            return Poll::Ready(Err(Error::ConnectionError(close_reason.clone())));
        }
        match ready!(inner.connection.notify_handshake_complete(cx)) {
            Ok(res) => {
                drop(inner);
                Poll::Ready(Ok((
                    res,
                    crate::Muxer(muxer.take().expect("polled after yielding Ready")),
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
