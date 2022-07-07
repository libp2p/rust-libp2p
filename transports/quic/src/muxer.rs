// Copyright 2020 Parity Technologies (UK) Ltd.
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

use crate::connection::{Connection, ConnectionEvent};
use crate::error::Error;

use futures::{AsyncRead, AsyncWrite};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    fmt, io,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll, Waker},
};

/// State for a single opened QUIC connection.
pub struct QuicMuxer {
    // Note: This could theoretically be an asynchronous future, in order to yield the current
    // task if a task running in parallel is already holding the lock. However, using asynchronous
    // mutexes without async/await is extremely tedious and maybe not worth the effort.
    inner: Arc<Mutex<QuicMuxerInner>>,
}

/// Mutex-protected fields of [`QuicMuxer`].
struct QuicMuxerInner {
    /// Inner connection object that yields events.
    connection: Connection,
    // /// State of all the substreams that the muxer reports as open.
    substreams: HashMap<quinn_proto::StreamId, SubstreamState>,
    /// A FIFO of wakers to wake if a new outgoing substream is opened.
    pending_substreams: VecDeque<Waker>,
    /// Waker to wake if the connection is closed.
    poll_close_waker: Option<Waker>,
    /// Waker to wake if any event is happened.
    poll_event_waker: Option<Waker>,
}

/// State of a single substream.
#[derive(Default, Clone)]
struct SubstreamState {
    /// Waker to wake if the substream becomes readable or stopped.
    read_waker: Option<Waker>,
    /// Waker to wake if the substream becomes writable or stopped.
    write_waker: Option<Waker>,
    /// True if the substream has been finished.
    finished: bool,
    /// True if the substream has been stopped.
    stopped: bool,
    /// Waker to wake if the substream becomes closed or stopped.
    finished_waker: Option<Waker>,
}

impl QuicMuxer {
    /// Crate-internal function that builds a [`QuicMuxer`] from a raw connection.
    ///
    /// # Panic
    ///
    /// Panics if `connection.is_handshaking()` returns `true`.
    pub(crate) fn from_connection(connection: Connection) -> Self {
        assert!(!connection.is_handshaking());

        QuicMuxer {
            inner: Arc::new(Mutex::new(QuicMuxerInner {
                connection: connection,
                substreams: Default::default(),
                pending_substreams: Default::default(),
                poll_close_waker: None,
                poll_event_waker: None,
            })),
        }
    }
}

pub struct Substream {
    id: quinn_proto::StreamId,
    muxer: Weak<Mutex<QuicMuxerInner>>,
}

impl Substream {
    fn new(id: quinn_proto::StreamId, muxer: Arc<Mutex<QuicMuxerInner>>) -> Self {
        Self {
            id,
            muxer: Arc::downgrade(&muxer),
        }
    }
}

impl Drop for Substream {
    fn drop(&mut self) {
        if let Some(muxer) = self.muxer.upgrade() {
            let mut muxer = muxer.lock();
            muxer.substreams.remove(&self.id);
        }
    }
}

impl AsyncRead for Substream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        use quinn_proto::{ReadError, ReadableError};
        use std::io::Write;

        let muxer = self
            .muxer
            .upgrade()
            .expect("StreamMuxer::read_substream: muxer is dead");
        let mut muxer = muxer.lock();

        let substream_state = muxer
            .substreams
            .get(&self.id)
            .expect("invalid Substream::poll_read API usage");
        if substream_state.stopped {
            return Poll::Ready(Ok(0));
        }

        let mut stream = muxer.connection.connection.recv_stream(self.id);
        let mut chunks = match stream.read(true) {
            Ok(chunks) => chunks,
            Err(ReadableError::UnknownStream) => {
                return Poll::Ready(Ok(0)); // FIXME This is a hack,
                                           // a rust client should close substream correctly
                                           // return Poll::Ready(Err(Self::Error::ExpiredStream))
            }
            Err(ReadableError::IllegalOrderedRead) => {
                panic!("Illegal ordered read can only happen if `stream.read(false)` is used.");
            }
        };
        let mut bytes = 0;
        let mut pending = false;
        loop {
            if buf.is_empty() {
                break;
            }
            match chunks.next(buf.len()) {
                Ok(Some(chunk)) => {
                    buf.write_all(&chunk.bytes).expect("enough buffer space");
                    bytes += chunk.bytes.len();
                }
                Ok(None) => {
                    break;
                }
                Err(ReadError::Reset(error_code)) => {
                    tracing::error!(
                        "substream {} was reset with error code {}",
                        self.id,
                        error_code
                    );
                    bytes = 0;
                    break;
                }
                Err(ReadError::Blocked) => {
                    pending = true;
                    break;
                }
            }
        }
        if chunks.finalize().should_transmit() {
            if let Some(waker) = muxer.poll_event_waker.take() {
                waker.wake();
            }
        }
        if pending && bytes == 0 {
            let mut substream_state = muxer
                .substreams
                .get_mut(&self.id)
                .expect("known substream; qed");
            substream_state.read_waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(Ok(bytes))
        }
    }
}

impl AsyncWrite for Substream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        use quinn_proto::WriteError;

        let muxer = self
            .muxer
            .upgrade()
            .expect("Substream::poll_write: muxer is dead");
        let mut muxer = muxer.lock();

        match muxer.connection.connection.send_stream(self.id).write(buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(WriteError::Blocked) => {
                let mut substream = muxer
                    .substreams
                    .get_mut(&self.id)
                    .expect("known substream; qed");
                substream.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(err @ WriteError::Stopped(_)) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
            }
            Err(WriteError::UnknownStream) => {
                tracing::error!(
                    "The application used a connection that is already being \
                    closed. This is a bug in the application or in libp2p."
                );
                Poll::Pending
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        // quinn doesn't support flushing, calling close will flush all substreams.
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let muxer = self
            .muxer
            .upgrade()
            .expect("Substream::poll_close: muxer is dead");
        let mut muxer = muxer.lock();
        let muxer = &mut *muxer;

        let mut substream_state = muxer
            .substreams
            .get_mut(&self.id)
            .expect("invalid Substream::poll_close API usage");
        if substream_state.finished {
            return Poll::Ready(Ok(()));
        }

        match muxer.connection.shutdown_substream(self.id) {
            Ok(()) => {
                substream_state.finished_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(err @ quinn_proto::FinishError::Stopped(_)) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
            }
            Err(quinn_proto::FinishError::UnknownStream) => {
                // Illegal usage of the API.
                debug_assert!(false);
                Poll::Ready(Ok(()))
                // Poll::Ready(Err(Error::ExpiredStream)) FIXME
            }
        }
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = ();
    type Substream = Substream;
    type Error = Error;

    /// Polls for a connection-wide event.
    ///
    /// This function behaves the same as a `Stream`.
    ///
    /// If `Pending` is returned, then the current task will be notified once the muxer
    /// is ready to be polled, similar to the API of `Stream::poll()`.
    /// Only the latest task that was used to call this method may be notified.
    ///
    /// It is permissible and common to use this method to perform background
    /// work, such as processing incoming packets and polling timers.
    ///
    /// An error can be generated if the connection has been closed.
    fn poll_event(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent<Self::Substream>, Self::Error>> {
        // We use `poll_event` to perform the background processing of the entire connection.
        let mut inner = self.inner.lock();

        while let Poll::Ready(event) = inner.connection.poll_event(cx) {
            match event {
                ConnectionEvent::Connected => {
                    tracing::error!("Unexpected Connected event on established QUIC connection");
                }
                ConnectionEvent::ConnectionLost(_) => {
                    if let Some(waker) = inner.poll_close_waker.take() {
                        waker.wake();
                    }
                    inner.connection.close();
                }

                ConnectionEvent::StreamOpened => {
                    if let Some(waker) = inner.pending_substreams.pop_front() {
                        waker.wake();
                    }
                }
                ConnectionEvent::StreamReadable(substream) => {
                    if let Some(substream) = inner.substreams.get_mut(&substream) {
                        if let Some(waker) = substream.read_waker.take() {
                            waker.wake();
                        }
                    }
                }
                ConnectionEvent::StreamWritable(substream) => {
                    if let Some(substream) = inner.substreams.get_mut(&substream) {
                        if let Some(waker) = substream.write_waker.take() {
                            waker.wake();
                        }
                    }
                }
                ConnectionEvent::StreamFinished(substream) => {
                    if let Some(substream) = inner.substreams.get_mut(&substream) {
                        substream.finished = true;
                        if let Some(waker) = substream.finished_waker.take() {
                            waker.wake();
                        }
                    }
                }
                ConnectionEvent::StreamStopped(substream) => {
                    if let Some(substream) = inner.substreams.get_mut(&substream) {
                        substream.stopped = true;
                    }
                }
                ConnectionEvent::StreamAvailable => {
                    // Handled below.
                }
            }
        }

        if let Some(substream_id) = inner.connection.pop_incoming_substream() {
            inner.substreams.insert(substream_id, Default::default());
            let substream = Substream::new(substream_id, self.inner.clone());
            Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(substream)))
        } else {
            inner.poll_event_waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    /// Opens a new outgoing substream, and produces the equivalent to a future that will be
    /// resolved when it becomes available.
    ///
    /// We provide the same handler to poll it by multiple tasks, which is done as a FIFO
    /// queue via `poll_outbound`.
    fn open_outbound(&self) -> Self::OutboundSubstream {}

    /// Polls the outbound substream.
    ///
    /// If `Pending` is returned, then the current task will be notified once the substream
    /// is ready to be polled, similar to the API of `Future::poll()`.
    fn poll_outbound(
        &self,
        cx: &mut Context<'_>,
        _: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut inner = self.inner.lock();
        if let Some(substream_id) = inner.connection.pop_outgoing_substream() {
            inner.substreams.insert(substream_id, Default::default());
            let substream = Substream::new(substream_id, self.inner.clone());
            Poll::Ready(Ok(substream))
        } else {
            inner.pending_substreams.push_back(cx.waker().clone());
            Poll::Pending
        }
    }

    /// Destroys an outbound substream future. Use this after the outbound substream has finished,
    /// or if you want to interrupt it.
    fn destroy_outbound(&self, _: Self::OutboundSubstream) {
        // Do nothing because we don't know which waker should be destroyed.
        // TODO `Self::OutboundSubstream` -> autoincrement id.
    }

    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut inner = self.inner.lock();

        if inner.connection.connection.is_drained() {
            return Poll::Ready(Ok(()));
        }

        if inner.substreams.is_empty() {
            let connection = &mut inner.connection;
            if !connection.connection.is_closed() {
                connection.close();
                if let Some(waker) = inner.poll_event_waker.take() {
                    waker.wake();
                }
            } else {
            }
            while let Poll::Ready(event) = inner.connection.poll_event(cx) {
                if let ConnectionEvent::ConnectionLost(_) = event {
                    return Poll::Ready(Ok(()));
                }
            }
        } else {
            for substream in inner.substreams.clone().keys() {
                if let Err(e) = inner.connection.shutdown_substream(*substream) {
                    tracing::error!("substream finish error on muxer close: {}", e);
                }
            }
        }

        // Register `cx.waker()` as being woken up if the connection closes.
        inner.poll_close_waker = Some(cx.waker().clone());

        Poll::Pending
    }
}

impl fmt::Debug for QuicMuxer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("QuicMuxer").finish()
    }
}
