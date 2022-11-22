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

mod connecting;
mod substream;

use crate::{
    endpoint::{self, ToEndpoint},
    Error,
};
pub use connecting::Connecting;
pub use substream::Substream;
use substream::SubstreamState;

use futures::{channel::mpsc, ready, FutureExt, StreamExt};
use futures_timer::Delay;
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use parking_lot::Mutex;
use std::{
    any::Any,
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Instant,
};

/// State for a single opened QUIC connection.
#[derive(Debug)]
pub struct Connection {
    /// State shared with the substreams.
    state: Arc<Mutex<State>>,
    /// Channel to the [`endpoint::Driver`] that drives the [`quinn_proto::Endpoint`] that
    /// this connection belongs to.
    endpoint_channel: endpoint::Channel,
    /// Pending message to be sent to the [`quinn_proto::Endpoint`] in the [`endpoint::Driver`].
    pending_to_endpoint: Option<ToEndpoint>,
    /// Events that the [`quinn_proto::Endpoint`] will send in destination to our local
    /// [`quinn_proto::Connection`].
    from_endpoint: mpsc::Receiver<quinn_proto::ConnectionEvent>,
    /// Identifier for this connection according to the [`quinn_proto::Endpoint`].
    /// Used when sending messages to the endpoint.
    connection_id: quinn_proto::ConnectionHandle,
    /// `Future` that triggers at the [`Instant`] that [`quinn_proto::Connection::poll_timeout`]
    /// indicates.
    next_timeout: Option<(Delay, Instant)>,
}

impl Connection {
    /// Build a [`Connection`] from raw components.
    ///
    /// This function assumes that there exists a [`Driver`](super::endpoint::Driver)
    /// that will process the messages sent to `EndpointChannel::to_endpoint` and send us messages
    /// on `from_endpoint`.
    ///
    /// `connection_id` is used to identify the local connection in the messages sent to
    /// `to_endpoint`.
    ///
    /// This function assumes that the [`quinn_proto::Connection`] is completely fresh and none of
    /// its methods has ever been called. Failure to comply might lead to logic errors and panics.
    pub(crate) fn from_quinn_connection(
        endpoint_channel: endpoint::Channel,
        connection: quinn_proto::Connection,
        connection_id: quinn_proto::ConnectionHandle,
        from_endpoint: mpsc::Receiver<quinn_proto::ConnectionEvent>,
    ) -> Self {
        let state = State {
            connection,
            substreams: HashMap::new(),
            poll_connection_waker: None,
            poll_inbound_waker: None,
            poll_outbound_waker: None,
        };
        Self {
            endpoint_channel,
            pending_to_endpoint: None,
            next_timeout: None,
            from_endpoint,
            connection_id,
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// The address that the local socket is bound to.
    pub(crate) fn local_addr(&self) -> &SocketAddr {
        self.endpoint_channel.socket_addr()
    }

    /// Returns the address of the node we're connected to.
    pub(crate) fn remote_addr(&self) -> SocketAddr {
        self.state.lock().connection.remote_address()
    }

    /// Identity of the remote peer inferred from the handshake.
    ///
    /// `None` if the handshake is not complete yet, i.e. [`Self::poll_event`]
    ///  has not yet reported a [`quinn_proto::Event::Connected`]
    fn peer_identity(&self) -> Option<Box<dyn Any>> {
        self.state
            .lock()
            .connection
            .crypto_session()
            .peer_identity()
    }

    /// Polls the connection for an event that happened on it.
    ///
    /// `quinn::proto::Connection` is polled in the order instructed in their docs:
    /// 1. [`quinn_proto::Connection::poll_transmit`]
    /// 2. [`quinn_proto::Connection::poll_timeout`]
    /// 3. [`quinn_proto::Connection::poll_endpoint_events`]
    /// 4. [`quinn_proto::Connection::poll`]
    fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Option<quinn_proto::Event>> {
        let mut inner = self.state.lock();
        loop {
            // Sending the pending event to the endpoint. If the endpoint is too busy, we just
            // stop the processing here.
            // We don't deliver substream-related events to the user as long as
            // `to_endpoint` is full. This should propagate the back-pressure of `to_endpoint`
            // being full to the user.
            if let Some(to_endpoint) = self.pending_to_endpoint.take() {
                match self.endpoint_channel.try_send(to_endpoint, cx) {
                    Ok(Ok(())) => {}
                    Ok(Err(to_endpoint)) => {
                        self.pending_to_endpoint = Some(to_endpoint);
                        return Poll::Pending;
                    }
                    Err(endpoint::Disconnected {}) => {
                        return Poll::Ready(None);
                    }
                }
            }

            match self.from_endpoint.poll_next_unpin(cx) {
                Poll::Ready(Some(event)) => {
                    inner.connection.handle_event(event);
                    continue;
                }
                Poll::Ready(None) => {
                    return Poll::Ready(None);
                }
                Poll::Pending => {}
            }

            // The maximum amount of segments which can be transmitted in a single Transmit
            // if a platform supports Generic Send Offload (GSO).
            // Set to 1 for now since not all platforms support GSO.
            // TODO: Fix for platforms that support GSO.
            let max_datagrams = 1;
            // Poll the connection for packets to send on the UDP socket and try to send them on
            // `to_endpoint`.
            if let Some(transmit) = inner
                .connection
                .poll_transmit(Instant::now(), max_datagrams)
            {
                // TODO: ECN bits not handled
                self.pending_to_endpoint = Some(ToEndpoint::SendUdpPacket(transmit));
                continue;
            }

            match inner.connection.poll_timeout() {
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
                    inner.connection.handle_timeout(*when);
                    continue;
                }
            }

            // The connection also needs to be able to send control messages to the endpoint. This is
            // handled here, and we try to send them on `to_endpoint` as well.
            if let Some(event) = inner.connection.poll_endpoint_events() {
                let connection_id = self.connection_id;
                self.pending_to_endpoint = Some(ToEndpoint::ProcessConnectionEvent {
                    connection_id,
                    event,
                });
                continue;
            }

            // The final step consists in returning the events related to the various substreams.
            if let Some(ev) = inner.connection.poll() {
                return Poll::Ready(Some(ev));
            }

            return Poll::Pending;
        }
    }
}

impl StreamMuxer for Connection {
    type Substream = Substream;
    type Error = Error;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        while let Poll::Ready(event) = self.poll_event(cx) {
            let mut inner = self.state.lock();
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
                    inner
                        .connection
                        .close(Instant::now(), From::from(0u32), Default::default());
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
                        substream.is_write_closed = true;
                        if let Some(waker) = substream.write_waker.take() {
                            waker.wake();
                        }
                        if let Some(waker) = substream.finished_waker.take() {
                            waker.wake();
                        }
                    }
                }
                quinn_proto::Event::Stream(quinn_proto::StreamEvent::Stopped {
                    id,
                    error_code: _,
                }) => {
                    if let Some(substream) = inner.substreams.get_mut(&id) {
                        if let Some(waker) = substream.write_waker.take() {
                            waker.wake();
                        }
                        if let Some(waker) = substream.finished_waker.take() {
                            waker.wake();
                        }
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

        self.state.lock().poll_connection_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut inner = self.state.lock();

        let substream_id = match inner.connection.streams().accept(quinn_proto::Dir::Bi) {
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
        let substream = Substream::new(substream_id, self.state.clone());

        Poll::Ready(Ok(substream))
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut inner = self.state.lock();
        let substream_id = match inner.connection.streams().open(quinn_proto::Dir::Bi) {
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
        let substream = Substream::new(substream_id, self.state.clone());
        Poll::Ready(Ok(substream))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut inner = self.state.lock();
        if inner.connection.is_drained() {
            return Poll::Ready(Ok(()));
        }

        for substream in inner.substreams.keys().cloned().collect::<Vec<_>>() {
            let _ = inner.connection.send_stream(substream).finish();
        }

        if inner.connection.streams().send_streams() == 0 && !inner.connection.is_closed() {
            inner
                .connection
                .close(Instant::now(), From::from(0u32), Default::default())
        }
        drop(inner);

        loop {
            match ready!(self.poll_event(cx)) {
                Some(quinn_proto::Event::ConnectionLost { .. }) => return Poll::Ready(Ok(())),
                None => return Poll::Ready(Err(Error::EndpointDriverCrashed)),
                _ => {}
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let to_endpoint = ToEndpoint::ProcessConnectionEvent {
            connection_id: self.connection_id,
            event: quinn_proto::EndpointEvent::drained(),
        };
        self.endpoint_channel.send_on_drop(to_endpoint);
    }
}

/// Mutex-protected state of [`Connection`].
#[derive(Debug)]
pub struct State {
    /// The QUIC inner state machine for this specific connection.
    connection: quinn_proto::Connection,

    /// State of all the substreams that the muxer reports as open.
    pub substreams: HashMap<quinn_proto::StreamId, SubstreamState>,

    /// Waker to wake if a new outbound substream is opened.
    pub poll_outbound_waker: Option<Waker>,
    /// Waker to wake if a new inbound substream was happened.
    pub poll_inbound_waker: Option<Waker>,
    /// Waker to wake if the connection should be polled again.
    pub poll_connection_waker: Option<Waker>,
}

impl State {
    fn unchecked_substream_state(&mut self, id: quinn_proto::StreamId) -> &mut SubstreamState {
        self.substreams
            .get_mut(&id)
            .expect("Substream should be known.")
    }
}
