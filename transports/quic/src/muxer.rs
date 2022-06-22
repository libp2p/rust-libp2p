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

use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    task::{Context, Poll, Waker},
};

pub type Substream = quinn_proto::StreamId;

/// State for a single opened QUIC connection.
pub struct QuicMuxer {
    // Note: This could theoretically be an asynchronous future, in order to yield the current
    // task if a task running in parallel is already holding the lock. However, using asynchronous
    // mutexes without async/await is extremely tedious and maybe not worth the effort.
    inner: Mutex<QuicMuxerInner>,
}

/// Mutex-protected fields of [`QuicMuxer`].
struct QuicMuxerInner {
    /// Inner connection object that yields events.
    connection: Connection,
    /// State of all the substreams that the muxer reports as open.
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
            inner: Mutex::new(QuicMuxerInner {
                connection,
                substreams: Default::default(),
                pending_substreams: Default::default(),
                poll_close_waker: None,
                poll_event_waker: None,
            }),
        }
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = ();
    type Substream = quinn_proto::StreamId;
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

        if let Some(substream) = inner.connection.pop_incoming_substream() {
            inner.substreams.insert(substream, Default::default());
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
        if let Some(substream) = inner.connection.pop_outgoing_substream() {
            inner.substreams.insert(substream, Default::default());
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

    /// Write data to a substream. The behaviour is the same as `futures::AsyncWrite::poll_write`.
    ///
    /// If `Pending` is returned, then the current task will be notified once the substream
    /// is ready to be read. For each individual substream, only the latest task that was used to
    /// call this method may be notified.
    ///
    /// Calling `write_substream` does not guarantee that data will arrive to the remote. To
    /// ensure that, you should call `flush_substream`.
    ///
    /// It is incorrect to call this method on a substream if you called `shutdown_substream` on
    /// this substream earlier.
    fn write_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::WriteError;

        let mut inner = self.inner.lock();

        let id = substream;

        match inner.connection.connection.send_stream(*id).write(buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(WriteError::Blocked) => {
                let mut substream = inner.substreams.get_mut(id).expect("known substream; qed");
                substream.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(WriteError::Stopped(err_code)) => Poll::Ready(Err(Error::Reset(err_code))),
            Err(WriteError::UnknownStream) => {
                tracing::error!(
                    "The application used a connection that is already being \
                    closed. This is a bug in the application or in libp2p."
                );
                Poll::Pending
            }
        }
    }

    /// Reads data from a substream. The behaviour is the same as `futures::AsyncRead::poll_read`.
    ///
    /// If `Pending` is returned, then the current task will be notified once the substream
    /// is ready to be read. However, for each individual substream, only the latest task that
    /// was used to call this method may be notified.
    ///
    /// If `Async::Ready(0)` is returned, the substream has been closed by the remote and should
    /// no longer be read afterwards.
    ///
    /// An error can be generated if the connection has been closed, or if a protocol misbehaviour
    /// happened.
    fn read_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
        mut buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::{ReadError, ReadableError};
        use std::io::Write;

        let id = *substream;

        let mut inner = self.inner.lock();

        let substream_state = inner
            .substreams
            .get_mut(substream)
            .expect("invalid StreamMuxer::read_substream API usage");
        if substream_state.stopped {
            return Poll::Ready(Ok(0));
        }

        let mut stream = inner.connection.connection.recv_stream(id);
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
                    tracing::error!("substream {} was reset with error code {}", id, error_code);
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
            if let Some(waker) = inner.poll_event_waker.take() {
                waker.wake();
            }
        }
        let substream = inner.substreams.get_mut(&id).expect("known substream; qed");
        if pending && bytes == 0 {
            substream.read_waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(Ok(bytes))
        }
    }

    fn shutdown_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        let mut inner = self.inner.lock();
        let inner = &mut *inner;

        let mut substream_state = inner
            .substreams
            .get_mut(substream)
            .expect("invalid StreamMuxer::shutdown_substream API usage");
        if substream_state.finished {
            return Poll::Ready(Ok(()));
        }

        match inner.connection.shutdown_substream(*substream) {
            Ok(()) => {
                substream_state.finished_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(quinn_proto::FinishError::Stopped(err)) => Poll::Ready(Err(Error::Reset(err))),
            Err(quinn_proto::FinishError::UnknownStream) => {
                // Illegal usage of the API.
                debug_assert!(false);
                Poll::Ready(Err(Error::ExpiredStream))
            }
        }
    }

    fn destroy_substream(&self, substream: Self::Substream) {
        let mut inner = self.inner.lock();
        inner.substreams.remove(&substream);
    }

    fn flush_substream(
        &self,
        _: &mut Context<'_>,
        _: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        // quinn doesn't support flushing, calling close will flush all substreams.
        Poll::Ready(Ok(()))
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
