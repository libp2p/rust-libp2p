// Copyright 2022 Protocol Labs.
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

use std::{
    io::{self, Write},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use futures::{AsyncRead, AsyncWrite};
use parking_lot::Mutex;

use super::State;

/// Wakers for the [`AsyncRead`] and [`AsyncWrite`] on a substream.
#[derive(Debug, Default, Clone)]
pub struct SubstreamState {
    /// Waker to wake if the substream becomes readable.
    pub read_waker: Option<Waker>,
    /// Waker to wake if the substream becomes writable, closed or stopped.
    pub write_waker: Option<Waker>,
    /// Waker to wake if the substream becomes closed or stopped.
    pub finished_waker: Option<Waker>,

    /// `true` if the stream finished, i.e. the writing side closed.
    pub is_write_closed: bool,
}

impl SubstreamState {
    /// Wake all wakers for reading, writing and closed the stream.
    pub fn wake_all(&mut self) {
        if let Some(waker) = self.read_waker.take() {
            waker.wake();
        }
        if let Some(waker) = self.write_waker.take() {
            waker.wake();
        }
        if let Some(waker) = self.finished_waker.take() {
            waker.wake();
        }
    }
}

/// A single stream on a connection
#[derive(Debug)]
pub struct Substream {
    /// The id of the stream.
    id: quinn_proto::StreamId,
    /// The state of the [`super::Connection`] this stream belongs to.
    state: Arc<Mutex<State>>,
}

impl Substream {
    pub fn new(id: quinn_proto::StreamId, state: Arc<Mutex<State>>) -> Self {
        Self { id, state }
    }
}

impl AsyncRead for Substream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.state.lock();

        let mut stream = state.connection.recv_stream(self.id);
        let mut chunks = match stream.read(true) {
            Ok(chunks) => chunks,
            Err(quinn_proto::ReadableError::UnknownStream) => {
                return Poll::Ready(Ok(0));
            }
            Err(quinn_proto::ReadableError::IllegalOrderedRead) => {
                unreachable!(
                    "Illegal ordered read can only happen if `stream.read(false)` is used."
                );
            }
        };

        let mut bytes = 0;
        let mut pending = false;
        let mut error = None;
        loop {
            if buf.is_empty() {
                // Chunks::next will continue returning `Ok(Some(_))` with an
                // empty chunk if there is no space left in the buffer, so we
                // break early here.
                break;
            }
            let chunk = match chunks.next(buf.len()) {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(err @ quinn_proto::ReadError::Reset(_)) => {
                    error = Some(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)));
                    break;
                }
                Err(quinn_proto::ReadError::Blocked) => {
                    pending = true;
                    break;
                }
            };

            buf.write_all(&chunk.bytes).expect("enough buffer space");
            bytes += chunk.bytes.len();
        }
        if chunks.finalize().should_transmit() {
            if let Some(waker) = state.poll_connection_waker.take() {
                waker.wake();
            }
        }
        if let Some(err) = error {
            return Poll::Ready(err);
        }

        if pending && bytes == 0 {
            let substream_state = state.unchecked_substream_state(self.id);
            substream_state.read_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        Poll::Ready(Ok(bytes))
    }
}

impl AsyncWrite for Substream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut state = self.state.lock();

        match state.connection.send_stream(self.id).write(buf) {
            Ok(bytes) => {
                if let Some(waker) = state.poll_connection_waker.take() {
                    waker.wake();
                }
                Poll::Ready(Ok(bytes))
            }
            Err(quinn_proto::WriteError::Blocked) => {
                let substream_state = state.unchecked_substream_state(self.id);
                substream_state.write_waker = Some(cx.waker().clone());
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
        let mut inner = self.state.lock();

        if inner.unchecked_substream_state(self.id).is_write_closed {
            return Poll::Ready(Ok(()));
        }

        match inner.connection.send_stream(self.id).finish() {
            Ok(()) => {
                let substream_state = inner.unchecked_substream_state(self.id);
                substream_state.finished_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(err @ quinn_proto::FinishError::Stopped(_)) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
            }
            Err(quinn_proto::FinishError::UnknownStream) => {
                // We never make up IDs so the stream must have existed at some point if we get to here.
                // `UnknownStream` is also emitted in case the stream is already finished, hence just
                // return `Ok(())` here.
                Poll::Ready(Ok(()))
            }
        }
    }
}

impl Drop for Substream {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        state.substreams.remove(&self.id);
        let _ = state.connection.recv_stream(self.id).stop(0u32.into());
        let mut send_stream = state.connection.send_stream(self.id);
        match send_stream.finish() {
            Ok(()) => {}
            // Already finished or reset, which is fine.
            Err(quinn_proto::FinishError::UnknownStream) => {}
            Err(quinn_proto::FinishError::Stopped(reason)) => {
                let _ = send_stream.reset(reason);
            }
        }
    }
}
