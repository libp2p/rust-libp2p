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

use crate::{connection::Connection, Error};

use futures::{ready, AsyncRead, AsyncWrite};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    io::{self, Write},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

/// State for a single opened QUIC connection.
#[derive(Debug)]
pub struct Muxer {
    inner: Arc<Mutex<Inner>>,
}

impl Muxer {
    /// Crate-internal function that builds a [`Muxer`] from a raw connection.
    pub(crate) fn new(inner: Inner) -> Self {
        Muxer {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl StreamMuxer for Muxer {
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
            let event = match event {
                Some(event) => event,
                None => return Poll::Ready(Err(Error::EndpointDriverCrashed)),
            };
            match event {
                quinn_proto::Event::Connected | quinn_proto::Event::HandshakeDataReady => {
                    debug_assert!(
                        false,
                        "Unexpected event {:?} on established QUIC connection",
                        event
                    );
                }
                quinn_proto::Event::ConnectionLost { reason } => {
                    inner.connection.close();
                    inner.substreams.values_mut().for_each(|s| s.wake_all());
                    return Poll::Ready(Err(Error::Connection(reason.into())));
                }
                quinn_proto::Event::Stream(quinn_proto::StreamEvent::Opened {
                    dir: quinn_proto::Dir::Bi,
                }) => {
                    if let Some(waker) = inner.poll_outbound_waker.take() {
                        waker.wake();
                    }
                }
                quinn_proto::Event::Stream(quinn_proto::StreamEvent::Available {
                    dir: quinn_proto::Dir::Bi,
                }) => {
                    if let Some(waker) = inner.poll_inbound_waker.take() {
                        waker.wake();
                    }
                }
                quinn_proto::Event::Stream(quinn_proto::StreamEvent::Readable { id }) => {
                    if let Some(substream) = inner.substreams.get_mut(&id) {
                        if let Some(waker) = substream.read_waker.take() {
                            waker.wake();
                        }
                    }
                }
                quinn_proto::Event::Stream(quinn_proto::StreamEvent::Writable { id }) => {
                    if let Some(substream) = inner.substreams.get_mut(&id) {
                        if let Some(waker) = substream.write_waker.take() {
                            waker.wake();
                        }
                    }
                }
                quinn_proto::Event::Stream(quinn_proto::StreamEvent::Finished { id }) => {
                    if let Some(substream) = inner.substreams.get_mut(&id) {
                        substream.wake_all();
                        substream.is_write_closed = true;
                    }
                }
                quinn_proto::Event::Stream(quinn_proto::StreamEvent::Stopped {
                    id,
                    error_code: _,
                }) => {
                    if let Some(substream) = inner.substreams.get_mut(&id) {
                        substream.wake_all();
                    }
                }
                quinn_proto::Event::DatagramReceived
                | quinn_proto::Event::Stream(quinn_proto::StreamEvent::Available {
                    dir: quinn_proto::Dir::Uni,
                })
                | quinn_proto::Event::Stream(quinn_proto::StreamEvent::Opened {
                    dir: quinn_proto::Dir::Uni,
                }) => {
                    unreachable!("We don't use datagrams or unidirectional streams.")
                }
            }
        }
        // TODO: If connection migration is enabled (currently disabled) address
        // change on the connection needs to be handled.

        inner.poll_connection_waker = Some(cx.waker().clone());
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

        for substream in substreams.keys() {
            let _ = connection.finish_substream(*substream);
        }

        loop {
            if connection.send_stream_count() == 0 && !connection.is_closed() {
                connection.close()
            }
            match ready!(connection.poll_event(cx)) {
                Some(quinn_proto::Event::ConnectionLost { .. }) => return Poll::Ready(Ok(())),
                None => return Poll::Ready(Err(Error::EndpointDriverCrashed)),
                _ => {}
            }
        }
    }
}

/// Mutex-protected fields of [`Muxer`].
#[derive(Debug)]
pub struct Inner {
    /// Inner connection object that yields events.
    pub connection: Connection,
    /// State of all the substreams that the muxer reports as open.
    pub substreams: HashMap<quinn_proto::StreamId, SubstreamState>,
    /// Waker to wake if a new outbound substream is opened.
    pub poll_outbound_waker: Option<Waker>,
    /// Waker to wake if a new inbound substream was happened.
    pub poll_inbound_waker: Option<Waker>,
    /// Waker to wake if the connection should be polled again.
    pub poll_connection_waker: Option<Waker>,
}

impl Inner {
    fn unchecked_substream_state(&mut self, id: quinn_proto::StreamId) -> &mut SubstreamState {
        self.substreams
            .get_mut(&id)
            .expect("Substream should be known.")
    }
}

/// State of a single substream.
#[derive(Debug, Default, Clone)]
pub struct SubstreamState {
    /// Waker to wake if the substream becomes readable or stopped.
    read_waker: Option<Waker>,
    /// Waker to wake if the substream becomes writable or stopped.
    write_waker: Option<Waker>,
    /// Waker to wake if the substream becomes closed or stopped.
    finished_waker: Option<Waker>,

    is_write_closed: bool,
}

impl SubstreamState {
    fn wake_all(&mut self) {
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

#[derive(Debug)]
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
        let mut muxer = self.muxer.lock();

        let mut stream = muxer.connection.recv_stream(self.id);
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
        loop {
            let chunk = match chunks.next(buf.len()) {
                Ok(Some(chunk)) if !chunk.bytes.is_empty() => chunk,
                Ok(_) => break,
                Err(err @ quinn_proto::ReadError::Reset(_)) => {
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
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
            if let Some(waker) = muxer.poll_connection_waker.take() {
                waker.wake();
            }
        }
        if pending && bytes == 0 {
            let substream_state = muxer.unchecked_substream_state(self.id);
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
            Ok(bytes) => {
                if let Some(waker) = muxer.poll_connection_waker.take() {
                    waker.wake();
                }
                Poll::Ready(Ok(bytes))
            }
            Err(quinn_proto::WriteError::Blocked) => {
                let substream_state = muxer.unchecked_substream_state(self.id);
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
        let mut muxer = self.muxer.lock();

        if muxer.unchecked_substream_state(self.id).is_write_closed {
            return Poll::Ready(Ok(()));
        }

        match muxer.connection.finish_substream(self.id) {
            Ok(()) => {
                let substream_state = muxer.unchecked_substream_state(self.id);
                substream_state.finished_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Err(err @ quinn_proto::FinishError::Stopped(_)) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, err)))
            }
            Err(quinn_proto::FinishError::UnknownStream) => {
                Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
            }
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
            Err(quinn_proto::FinishError::UnknownStream) => {}
            Err(quinn_proto::FinishError::Stopped(reason)) => {
                let _ = send_stream.reset(reason);
            }
        }
    }
}
