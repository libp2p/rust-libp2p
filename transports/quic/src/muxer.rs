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

use crate::{
    endpoint::{self, ToEndpoint},
    Error,
};

use futures::{channel::mpsc, ready, AsyncRead, AsyncWrite, FutureExt, StreamExt};
use futures_timer::Delay;
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    io::{self, Write},
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Instant,
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
        while let Poll::Ready(event) = inner.poll_event(cx) {
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
                    inner.close();
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

        let substream_id = match inner.accept_substream() {
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
        let substream_id = match inner.open_substream() {
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
        let inner = &mut *self.inner.lock();
        if inner.is_drained() {
            return Poll::Ready(Ok(()));
        }

        for substream in inner.substreams.keys().cloned().collect::<Vec<_>>() {
            let _ = inner.finish_substream(substream);
        }

        loop {
            if inner.send_stream_count() == 0 && !inner.is_closed() {
                inner.close()
            }
            match ready!(inner.poll_event(cx)) {
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
    /// Channel to the endpoint this connection belongs to.
    endpoint_channel: endpoint::Channel,
    /// Pending message to be sent to the background task that is driving the endpoint.
    pending_to_endpoint: Option<ToEndpoint>,
    /// Events that the endpoint will send in destination to our local [`quinn_proto::Connection`].
    /// Passed at initialization.
    from_endpoint: mpsc::Receiver<quinn_proto::ConnectionEvent>,

    /// The QUIC state machine for this specific connection.
    connection: quinn_proto::Connection,
    /// Identifier for this connection according to the endpoint. Used when sending messages to
    /// the endpoint.
    connection_id: quinn_proto::ConnectionHandle,
    /// `Future` that triggers at the [`Instant`] that `self.connection.poll_timeout()` indicates.
    next_timeout: Option<(Delay, Instant)>,

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

    /// Crate-internal function that builds [`Inner`] from raw components.
    ///
    /// This function assumes that there exists a [`EndpointDriver`](super::endpoint::EndpointDriver)
    /// that will process the messages sent to `EndpointChannel::to_endpoint` and send us messages
    /// on `from_endpoint`.
    ///
    /// `connection_id` is used to identify the local connection in the messages sent to
    /// `to_endpoint`.
    ///
    /// This function assumes that the [`quinn_proto::Connection`] is completely fresh and none of
    /// its methods has ever been called. Failure to comply might lead to logic errors and panics.
    pub fn from_quinn_connection(
        endpoint_channel: endpoint::Channel,
        connection: quinn_proto::Connection,
        connection_id: quinn_proto::ConnectionHandle,
        from_endpoint: mpsc::Receiver<quinn_proto::ConnectionEvent>,
    ) -> Self {
        debug_assert!(!connection.is_closed());
        Inner {
            endpoint_channel,
            pending_to_endpoint: None,
            connection,
            next_timeout: None,
            from_endpoint,
            connection_id,
            substreams: HashMap::new(),
            poll_connection_waker: None,
            poll_inbound_waker: None,
            poll_outbound_waker: None,
        }
    }

    /// The address that the local socket is bound to.
    pub fn local_addr(&self) -> &SocketAddr {
        self.endpoint_channel.socket_addr()
    }

    /// Returns the address of the node we're connected to.
    pub fn remote_addr(&self) -> SocketAddr {
        self.connection.remote_address()
    }

    /// Start closing the connection. A [`quinn_proto::Event::ConnectionLost`] event will be
    /// produced in the future.
    pub fn close(&mut self) {
        // We send a dummy `0` error code with no message, as the API of StreamMuxer doesn't
        // support this.
        self.connection
            .close(Instant::now(), From::from(0u32), Default::default());
    }

    /// Whether the connection is closed.
    /// A [`quinn_proto::Event::ConnectionLost`] event is emitted with details when the
    /// connection becomes closed.
    pub fn is_closed(&self) -> bool {
        self.connection.is_closed()
    }

    /// Whether there is no longer any need to keep the connection around.
    /// All drained connections have been closed.
    pub fn is_drained(&self) -> bool {
        self.connection.is_drained()
    }

    /// Pops a new substream opened by the remote.
    ///
    /// If `None` is returned, then a [`quinn_proto::StreamEvent::Available`] event will later be
    /// produced when a substream is available.
    pub fn accept_substream(&mut self) -> Option<quinn_proto::StreamId> {
        self.connection.streams().accept(quinn_proto::Dir::Bi)
    }

    /// Pops a new substream opened locally.
    ///
    /// The API can be thought as if outgoing substreams were automatically opened by the local
    /// QUIC connection and were added to a queue for availability.
    ///
    /// If `None` is returned, then a [`quinn_proto::StreamEvent::Opened`] event will later be
    /// produced when a substream is available.
    pub fn open_substream(&mut self) -> Option<quinn_proto::StreamId> {
        self.connection.streams().open(quinn_proto::Dir::Bi)
    }

    /// Number of streams that may have unacknowledged data.
    pub fn send_stream_count(&mut self) -> usize {
        self.connection.streams().send_streams()
    }

    /// Closes the given substream.
    ///
    /// `write_substream` must no longer be called. The substream is however still
    /// readable.
    ///
    /// On success, a [`quinn_proto::StreamEvent::Finished`] event will later be produced when the
    /// substream has been effectively closed. A [`quinn_proto::StreamEvent::Stopped`] event can also
    /// be emitted.
    pub fn finish_substream(
        &mut self,
        id: quinn_proto::StreamId,
    ) -> Result<(), quinn_proto::FinishError> {
        self.connection.send_stream(id).finish()
    }

    pub fn crypto_session(&self) -> &dyn quinn_proto::crypto::Session {
        self.connection.crypto_session()
    }

    /// Polls the connection for an event that happened on it.
    pub fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Option<quinn_proto::Event>> {
        loop {
            match self.from_endpoint.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => {
                    self.connection.handle_event(event);
                    continue;
                }
                Poll::Ready(None) => {
                    return Poll::Ready(None);
                }
                Poll::Pending => {}
            }

            // Sending the pending event to the endpoint. If the endpoint is too busy, we just
            // stop the processing here.
            // We need to be careful to avoid a potential deadlock if both `from_endpoint` and
            // `to_endpoint` are full. As such, we continue to transfer data from `from_endpoint`
            // to the `quinn_proto::Connection` (see above).
            // However we don't deliver substream-related events to the user as long as
            // `to_endpoint` is full. This should propagate the back-pressure of `to_endpoint`
            // being full to the user.
            if let Some(to_endpoint) = self.pending_to_endpoint.take() {
                match self.endpoint_channel.try_send(to_endpoint, cx) {
                    Ok(Ok(())) => continue, // The endpoint may send back an event.
                    Ok(Err(to_endpoint)) => {
                        self.pending_to_endpoint = Some(to_endpoint);
                        return Poll::Pending;
                    }
                    Err(endpoint::Disconnected {}) => {
                        return Poll::Ready(None);
                    }
                }
            }

            // The maximum amount of segments which can be transmitted in a single Transmit
            // if a platform supports Generic Send Offload (GSO).
            // Set to 1 for now since not all platforms support GSO.
            // TODO: Fix for platforms that support GSO.
            let max_datagrams = 1;
            // Poll the connection for packets to send on the UDP socket and try to send them on
            // `to_endpoint`.
            if let Some(transmit) = self.connection.poll_transmit(Instant::now(), max_datagrams) {
                // TODO: ECN bits not handled
                self.pending_to_endpoint = Some(ToEndpoint::SendUdpPacket(transmit));
                continue;
            }

            match self.connection.poll_timeout() {
                Some(timeout) => match self.next_timeout {
                    Some((_, when)) if when == timeout => {}
                    _ => {
                        let now = Instant::now();
                        // 0ns if now > when
                        let duration = timeout.duration_since(now);
                        let next_timeout = Delay::new(duration);
                        self.next_timeout = Some((next_timeout, timeout))
                    }
                },
                None => self.next_timeout = None,
            }

            if let Some((timeout, when)) = self.next_timeout.as_mut() {
                if timeout.poll_unpin(cx).is_ready() {
                    self.connection.handle_timeout(*when);
                    continue;
                }
            }

            // The connection also needs to be able to send control messages to the endpoint. This is
            // handled here, and we try to send them on `to_endpoint` as well.
            if let Some(event) = self.connection.poll_endpoint_events() {
                let connection_id = self.connection_id;
                self.pending_to_endpoint = Some(ToEndpoint::ProcessConnectionEvent {
                    connection_id,
                    event,
                });
                continue;
            }

            // The final step consists in handling the events related to the various substreams.
            if let Some(ev) = self.connection.poll() {
                return Poll::Ready(Some(ev));
            }

            return Poll::Pending;
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let to_endpoint = ToEndpoint::ProcessConnectionEvent {
            connection_id: self.connection_id,
            event: quinn_proto::EndpointEvent::drained(),
        };
        self.endpoint_channel.send_on_drop(to_endpoint);
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

        match muxer.finish_substream(self.id) {
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
