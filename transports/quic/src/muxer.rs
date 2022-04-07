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
    collections::HashMap,
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
    /// Waker to wake if a new outgoing substream is opened.
    poll_substream_opened_waker: Option<Waker>,
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
    /// True if the substream has been closed.
    finished: bool,
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
                poll_substream_opened_waker: None,
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
                ConnectionEvent::ConnectionLost(e) => {
                    if let Some(waker) = inner.poll_close_waker.take() {
                        waker.wake();
                    }
                    inner.connection.close();
                    // return Poll::Ready(Err(Error::ConnectionLost(e)))
                }

                ConnectionEvent::StreamOpened => {
                    if let Some(waker) = inner.poll_substream_opened_waker.take() {
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
                        // if let ConnectionEvent::StreamFinished(_) = event {
                        substream.finished = true;
                        // }
                        // if let Some(waker) = substream.read_waker.take() {
                        //     waker.wake();
                        // }
                        // if let Some(waker) = substream.write_waker.take() {
                        //     waker.wake();
                        // }
                        if let Some(waker) = substream.finished_waker.take() {
                            waker.wake();
                        }
                    }
                }
                ConnectionEvent::StreamStopped(substream) => {
                    if let Some(substream) = inner.substreams.get_mut(&substream) {
                        substream.stopped = true;
                        // if let ConnectionEvent::StreamFinished(_) = event {
                        // substream.finished = true;
                        // }
                        // if let Some(waker) = substream.read_waker.take() {
                        //     waker.wake();
                        // }
                        // if let Some(waker) = substream.write_waker.take() {
                        //     waker.wake();
                        // }
                        // if let Some(waker) = substream.finished_waker.take() {
                        //     waker.wake();
                        // }
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

    fn open_outbound(&self) -> Self::OutboundSubstream {
        ()
    }

    // TODO: what if called multiple times? register all wakers?
    #[tracing::instrument(skip_all)]
    fn poll_outbound(
        &self,
        cx: &mut Context<'_>,
        _: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        // Note: this implementation makes it possible to poll the same `Self::OutboundSubstream`
        // over and over again and get new substreams. Using the API this way is invalid and would
        // normally result in a panic, but we decide to just ignore this problem.
        let mut inner = self.inner.lock();
        if let Some(substream) = inner.connection.pop_outgoing_substream() {
            inner.substreams.insert(substream, Default::default());
            return Poll::Ready(Ok(substream));
        }

        // Register `cx.waker()` as having to be woken up once a substream is available.
        if !inner
            .poll_substream_opened_waker
            .as_ref()
            .map_or(false, |w| w.will_wake(cx.waker()))
        {
            inner.poll_substream_opened_waker = Some(cx.waker().clone());
        }

        Poll::Pending
    }

    fn destroy_outbound(&self, _: Self::OutboundSubstream) {}

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
            Err(WriteError::Stopped(_)) => Poll::Ready(Ok(0)),
            Err(WriteError::UnknownStream) => Poll::Ready(Err(Self::Error::ExpiredStream)),
        }

        // match inner.connection.write_substream(*substream, buf) {
        //     Ok(bytes) => Poll::Ready(Ok(bytes)),
        //     Err(quinn_proto::WriteError::Stopped(err_code)) => {
        //         Poll::Ready(Err(Error::Reset(err_code)))
        //     },
        //     Err(quinn_proto::WriteError::Blocked) => {
        //         if let Some(substream) = inner.substreams.get_mut(substream) {
        //             if !substream
        //                 .write_waker
        //                 .as_ref()
        //                 .map_or(false, |w| w.will_wake(cx.waker()))
        //             {
        //                 substream.write_waker = Some(cx.waker().clone());
        //             }
        //         }
        //         Poll::Pending
        //     }
        //     Err(quinn_proto::WriteError::UnknownStream) => {
        //         tracing::error!(
        //             "The application used a connection that is already being \
        //             closed. This is a bug in the application or in libp2p."
        //         );
        //         Poll::Pending
        //     }
        // }
    }

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

    fn flush_all(&self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // quinn doesn't support flushing, calling close will flush all substreams.
        Poll::Ready(Ok(()))
    }

    fn close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // return Poll::Ready(Ok(()));
        // StreamMuxer's `close` documentation mentions that it automatically implies `flush_all`.
        if let Poll::Pending = self.flush_all(cx)? {
            return Poll::Pending;
        }

        // TODO: poll if closed or something

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
            tracing::error!(
                is_closed = inner.connection.connection.is_closed(),
                is_drained = inner.connection.connection.is_drained()
            );
            while let Poll::Ready(event) = inner.connection.poll_event(cx) {
                match event {
                    ConnectionEvent::ConnectionLost(_) => return Poll::Ready(Ok(())),
                    _ => {}
                }
            }
            // return Poll::Ready(Ok(()))
        } else {
            for substream in inner.substreams.clone().keys() {
                inner.connection.shutdown_substream(*substream);
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
