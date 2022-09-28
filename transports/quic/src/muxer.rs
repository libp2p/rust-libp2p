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
use quinn_proto::FinishError;
use std::{
    collections::HashMap,
    io::{self, Write},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

/// State for a single opened QUIC connection.
#[derive(Debug)]
pub struct QuicMuxer {
    // Note: This could theoretically be an asynchronous future, in order to yield the current
    // task if a task running in parallel is already holding the lock. However, using asynchronous
    // mutexes without async/await is extremely tedious and maybe not worth the effort.
    inner: Arc<Mutex<Inner>>,
}

/// Mutex-protected fields of [`QuicMuxer`].
#[derive(Debug)]
struct Inner {
    /// Inner connection object that yields events.
    connection: Connection,
    // /// State of all the substreams that the muxer reports as open.
    substreams: HashMap<quinn_proto::StreamId, SubstreamState>,
    /// Waker to wake if a new outbound substream is opened.
    poll_outbound_waker: Option<Waker>,
    /// Waker to wake if a new inbound substream was happened.
    poll_inbound_waker: Option<Waker>,
    /// Waker to wake if the connection should be polled again.
    poll_connection_waker: Option<Waker>,
}

/// State of a single substream.
#[derive(Debug, Default, Clone)]
struct SubstreamState {
    /// Waker to wake if the substream becomes readable or stopped.
    read_waker: Option<Waker>,
    /// Waker to wake if the substream becomes writable or stopped.
    write_waker: Option<Waker>,
    /// Waker to wake if the substream becomes closed or stopped.
    finished_waker: Option<Waker>,
}

impl QuicMuxer {
    /// Crate-internal function that builds a [`QuicMuxer`] from a raw connection.
    pub(crate) fn from_connection(connection: Connection) -> Self {
        QuicMuxer {
            inner: Arc::new(Mutex::new(Inner {
                connection,
                substreams: Default::default(),
                poll_outbound_waker: None,
                poll_inbound_waker: None,
                poll_connection_waker: None,
            })),
        }
    }
}

impl StreamMuxer for QuicMuxer {
    type Substream = Substream;
    type Error = Error;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        let mut inner = self.inner.lock();
        // Poll the inner [`quinn_proto::Connection`] for events and wake
        // the wakers of related poll-based methods.
        while let Poll::Ready(event) = inner.connection.poll_event(cx) {
            match event {
                ConnectionEvent::Connected | ConnectionEvent::HandshakeDataReady => {
                    debug_assert!(
                        false,
                        "Unexpected event {:?} on established QUIC connection",
                        event
                    );
                }
                ConnectionEvent::ConnectionLost(err) => {
                    return Poll::Ready(Err(Error::ConnectionLost(err)))
                }
                ConnectionEvent::StreamOpened => {
                    if let Some(waker) = inner.poll_outbound_waker.take() {
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
                ConnectionEvent::StreamFinished(substream)
                | ConnectionEvent::StreamStopped(substream) => {
                    if let Some(substream) = inner.substreams.get_mut(&substream) {
                        if let Some(waker) = substream.read_waker.take() {
                            waker.wake();
                        }
                        if let Some(waker) = substream.write_waker.take() {
                            waker.wake();
                        }
                        if let Some(waker) = substream.finished_waker.take() {
                            waker.wake();
                        }
                    }
                }
                ConnectionEvent::StreamAvailable => {
                    if let Some(waker) = inner.poll_inbound_waker.take() {
                        waker.wake();
                    }
                }
            }
        }
        inner.poll_connection_waker = Some(cx.waker().clone());

        // TODO: If connection migration is enabled (currently disabled) address
        // change on the connection needs to be handled.

        Poll::Pending
    }

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut inner = self.inner.lock();
        let substream_id = match inner.connection.accept_substream() {
            Some(id) => {
                inner.poll_outbound_waker = None;
                id
            }
            None => {
                inner.poll_inbound_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        };
        inner.substreams.insert(substream_id, Default::default());
        let substream = Substream::new(substream_id, self.inner.clone());
        Poll::Ready(Ok(substream))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut inner = self.inner.lock();
        let substream_id = match inner.connection.open_substream() {
            Some(id) => {
                inner.poll_outbound_waker = None;
                id
            }
            None => {
                inner.poll_outbound_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        };
        inner.substreams.insert(substream_id, Default::default());
        let substream = Substream::new(substream_id, self.inner.clone());
        Poll::Ready(Ok(substream))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let Inner {
            substreams,
            connection,
            ..
        } = &mut *self.inner.lock();
        if connection.is_drained() {
            return Poll::Ready(Ok(()));
        }

        if connection.send_stream_count() != 0 {
            for substream in substreams.keys() {
                if let Err(e) = connection.finish_substream(*substream) {
                    tracing::warn!("substream finish error on muxer close: {}", e);
                }
            }
        }
        loop {
            if connection.send_stream_count() == 0 && !connection.is_closed() {
                connection.close()
            }
            match connection.poll_event(cx) {
                Poll::Ready(ConnectionEvent::ConnectionLost(_)) => return Poll::Ready(Ok(())),
                Poll::Ready(_) => {}
                Poll::Pending => break,
            }
        }
        Poll::Pending
    }
}

pub struct Substream {
    id: quinn_proto::StreamId,
    muxer: Arc<Mutex<Inner>>,
}

impl Substream {
    fn new(id: quinn_proto::StreamId, muxer: Arc<Mutex<Inner>>) -> Self {
        Self { id, muxer }
    }
}

impl AsyncRead for Substream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        use quinn_proto::{ReadError, ReadableError};
        let mut muxer = self.muxer.lock();

        let mut stream = muxer.connection.recv_stream(self.id);
        let mut chunks = match stream.read(true) {
            Ok(chunks) => chunks,
            Err(ReadableError::UnknownStream) => {
                return Poll::Ready(Ok(0));
            }
            Err(ReadableError::IllegalOrderedRead) => {
                unreachable!(
                    "Illegal ordered read can only happen if `stream.read(false)` is used."
                );
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
                Ok(None) => break,
                Err(err @ ReadError::Reset(_)) => {
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
                }
                Err(ReadError::Blocked) => {
                    pending = true;
                    break;
                }
            }
        }
        if chunks.finalize().should_transmit() {
            if let Some(waker) = muxer.poll_connection_waker.take() {
                waker.wake();
            }
        }
        if pending && bytes == 0 {
            let substream_state = muxer
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
        let mut muxer = self.muxer.lock();

        match muxer.connection.send_stream(self.id).write(buf) {
            Ok(bytes) => Poll::Ready(Ok(bytes)),
            Err(quinn_proto::WriteError::Blocked) => {
                let substream = muxer
                    .substreams
                    .get_mut(&self.id)
                    .expect("known substream; qed");
                substream.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(err @ quinn_proto::WriteError::Stopped(_)) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
            }
            Err(quinn_proto::WriteError::UnknownStream) => {
                Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        // quinn doesn't support flushing, calling close will flush all substreams.
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut muxer = self.muxer.lock();
        match muxer.connection.finish_substream(self.id) {
            Ok(()) => {
                let substream_state = muxer
                    .substreams
                    .get_mut(&self.id)
                    .expect("Substream is not finished.");
                substream_state.finished_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(err @ quinn_proto::FinishError::Stopped(_)) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
            }
            Err(quinn_proto::FinishError::UnknownStream) => Poll::Ready(Ok(())),
        }
    }
}

impl Drop for Substream {
    fn drop(&mut self) {
        let mut muxer = self.muxer.lock();
        muxer.substreams.remove(&self.id);
        let _ = muxer.connection.recv_stream(self.id).stop(0u32.into());
        let mut send_stream = muxer.connection.send_stream(self.id);
        match send_stream.finish() {
            Ok(()) => {}
            // Already finished or reset, which is fine.
            Err(FinishError::UnknownStream) => {}
            Err(FinishError::Stopped(reason)) => {
                let _ = send_stream.reset(reason);
            }
        }
    }
}
