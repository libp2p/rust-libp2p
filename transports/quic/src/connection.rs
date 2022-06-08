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

//! A single QUIC connection.
//!
//! The [`Connection`] struct of this module contains, amongst other things, a
//! [`quinn_proto::Connection`] state machine and an `Arc<Endpoint>`. This struct is responsible
//! for communication between quinn_proto's connection and its associated endpoint.
//! All interactions with a QUIC connection should be done through this struct.
// TODO: docs

use crate::endpoint::Endpoint;

use async_io::Timer;
use futures::{channel::mpsc, prelude::*};
use libp2p_core::PeerId;
use std::{
    fmt,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};

/// Underlying structure for both [`crate::QuicMuxer`] and [`crate::Upgrade`].
///
/// Contains everything needed to process a connection with a remote.
/// Tied to a specific [`crate::Endpoint`].
pub(crate) struct Connection {
    /// Endpoint this connection belongs to.
    endpoint: Arc<Endpoint>,
    /// Future whose job is to send a message to the endpoint. Only one at a time.
    pending_to_endpoint: Option<Pin<Box<dyn Future<Output = ()> + Send + Sync>>>,
    /// Events that the endpoint will send in destination to our local [`quinn_proto::Connection`].
    /// Passed at initialization.
    from_endpoint: mpsc::Receiver<quinn_proto::ConnectionEvent>,

    /// The QUIC state machine for this specific connection.
    pub(crate) connection: quinn_proto::Connection,
    /// Identifier for this connection according to the endpoint. Used when sending messages to
    /// the endpoint.
    connection_id: quinn_proto::ConnectionHandle,
    /// `Future` that triggers at the `Instant` that `self.connection.poll_timeout()` indicates.
    next_timeout: Option<Timer>,

    /// In other to avoid race conditions where a "connected" event happens if we were not
    /// handshaking, we cache whether the connection is handshaking and only set this to true
    /// after a "connected" event has been received.
    ///
    /// In other words, this flag indicates whether a "connected" hasn't been received yet.
    is_handshaking: bool,
    /// Contains a `Some` if the connection is closed, with the reason of the closure.
    /// Contains `None` if it is still open.
    /// Contains `Some` if and only if a `ConnectionLost` event has been emitted.
    closed: Option<Error>,
}

/// Error on the connection as a whole.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Endpoint has force-killed this connection because it was too busy.
    #[error("Endpoint has force-killed our connection")]
    ClosedChannel,
    /// Error in the inner state machine.
    #[error("{0}")]
    Quinn(#[from] quinn_proto::ConnectionError),
}

impl Connection {
    /// Crate-internal function that builds a [`Connection`] from raw components.
    ///
    /// This function assumes that there exists a background task that will process the messages
    /// sent to `to_endpoint` and send us messages on `from_endpoint`.
    ///
    /// The `from_endpoint` can be purposefully closed by the endpoint if the connection is too
    /// slow to process.
    // TODO: is this necessary ^? figure out if quinn_proto doesn't forbid that situation in the first place
    ///
    /// `connection_id` is used to identify the local connection in the messages sent to
    /// `to_endpoint`.
    ///
    /// This function assumes that the [`quinn_proto::Connection`] is completely fresh and none of
    /// its methods has ever been called. Failure to comply might lead to logic errors and panics.
    // TODO: maybe abstract `to_endpoint` more and make it generic? dunno
    pub(crate) fn from_quinn_connection(
        endpoint: Arc<Endpoint>,
        connection: quinn_proto::Connection,
        connection_id: quinn_proto::ConnectionHandle,
        from_endpoint: mpsc::Receiver<quinn_proto::ConnectionEvent>,
    ) -> Self {
        assert!(!connection.is_closed());
        let is_handshaking = connection.is_handshaking();

        Connection {
            endpoint,
            pending_to_endpoint: None,
            connection,
            next_timeout: None,
            from_endpoint,
            connection_id,
            is_handshaking,
            closed: None,
        }
    }

    /// The local address which was used when the peer established the connection.
    ///
    /// Works for server connections only.
    pub(crate) fn local_addr(&self) -> SocketAddr {
        debug_assert_eq!(self.connection.side(), quinn_proto::Side::Server);
        let endpoint_addr = self.endpoint.local_addr;
        self.connection
            .local_ip()
            .map(|ip| SocketAddr::new(ip, endpoint_addr.port()))
            .unwrap_or_else(|| {
                // In a normal case scenario this should not happen, because
                // we get want to get a local addr for a server connection only.
                tracing::error!("trying to get quinn::local_ip for a client");
                endpoint_addr
            })
    }

    /// Returns the address of the node we're connected to.
    // TODO: can change /!\
    pub(crate) fn remote_addr(&self) -> SocketAddr {
        self.connection.remote_address()
    }

    /// Returns `true` if this connection is still pending. Returns `false` if we are connected to
    /// the remote or if the connection is closed.
    pub(crate) fn is_handshaking(&self) -> bool {
        self.is_handshaking
    }

    /// Returns the address of the node we're connected to.
    /// Panics if the connection is still handshaking.
    pub(crate) fn remote_peer_id(&self) -> PeerId {
        debug_assert!(!self.is_handshaking());
        let session = self.connection.crypto_session();
        let identity = session
            .peer_identity()
            .expect("connection got identity because it passed TLS handshake; qed");
        let certificates: Box<Vec<rustls::Certificate>> =
            identity.downcast().expect("we rely on rustls feature; qed");
        let end_entity = certificates
            .get(0)
            .expect("there should be exactly one certificate; qed");
        let end_entity_der = end_entity.as_ref();
        let p2p_cert = crate::tls::certificate::parse_certificate(end_entity_der)
            .expect("the certificate was validated during TLS handshake; qed");
        PeerId::from_public_key(&p2p_cert.extension.public_key)
    }

    /// Start closing the connection. A [`ConnectionEvent::ConnectionLost`] event will be
    /// produced in the future.
    pub(crate) fn close(&mut self) {
        // TODO: what if the user calls this multiple times?
        // We send a dummy `0` error code with no message, as the API of StreamMuxer doesn't
        // support this.
        self.connection
            .close(Instant::now(), From::from(0u32), Default::default());
        self.endpoint.report_quinn_event_non_block(
            self.connection_id,
            quinn_proto::EndpointEvent::drained(),
        );
    }

    /// Pops a new substream opened by the remote.
    ///
    /// If `None` is returned, then a [`ConnectionEvent::StreamAvailable`] event will later be
    /// produced when a substream is available.
    pub(crate) fn pop_incoming_substream(&mut self) -> Option<quinn_proto::StreamId> {
        self.connection.streams().accept(quinn_proto::Dir::Bi)
    }

    /// Pops a new substream opened locally.
    ///
    /// The API can be thought as if outgoing substreams were automatically opened by the local
    /// QUIC connection and were added to a queue for availability.
    ///
    /// If `None` is returned, then a [`ConnectionEvent::StreamOpened`] event will later be
    /// produced when a substream is available.
    pub(crate) fn pop_outgoing_substream(&mut self) -> Option<quinn_proto::StreamId> {
        self.connection.streams().open(quinn_proto::Dir::Bi)
    }

    /// Closes the given substream.
    ///
    /// [`Connection::write_substream`] must no longer be called. The substream is however still
    /// readable.
    ///
    /// On success, a [`StreamEvent::Finished`] event will later be produced when the
    /// substream has been effectively closed. A [`ConnectionEvent::StreamStopped`] event can also
    /// be emitted.
    pub(crate) fn shutdown_substream(
        &mut self,
        id: quinn_proto::StreamId,
    ) -> Result<(), quinn_proto::FinishError> {
        // closes the write end of the substream without waiting for the remote to receive the
        // event. use flush substream to wait for the remote to receive the event.
        self.connection.send_stream(id).finish()
    }

    /// Polls the connection for an event that happend on it.
    pub(crate) fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<ConnectionEvent> {
        // Nothing more can be done if the connection is closed.
        // Return `Pending` without registering the waker, essentially freezing the task forever.
        if self.closed.is_some() {
            return Poll::Pending;
        }

        // Process events that the endpoint has sent to us.
        loop {
            match Pin::new(&mut self.from_endpoint).poll_next(cx) {
                Poll::Ready(Some(event)) => self.connection.handle_event(event),
                Poll::Ready(None) => {
                    debug_assert!(self.closed.is_none());
                    let err = Error::ClosedChannel;
                    self.closed = Some(err.clone());
                    return Poll::Ready(ConnectionEvent::ConnectionLost(err));
                }
                Poll::Pending => break,
            }
        }

        'send_pending: loop {
            // Sending the pending event to the endpoint. If the endpoint is too busy, we just
            // stop the processing here.
            // There is a bit of a question in play here: should we continue to accept events
            // through `from_endpoint` if `to_endpoint` is busy?
            // We need to be careful to avoid a potential deadlock if both `from_endpoint` and
            // `to_endpoint` are full. As such, we continue to transfer data from `from_endpoint`
            // to the `quinn_proto::Connection` (see above).
            // However we don't deliver substream-related events to the user as long as
            // `to_endpoint` is full. This should propagate the back-pressure of `to_endpoint`
            // being full to the user.
            if let Some(pending_to_endpoint) = &mut self.pending_to_endpoint {
                match Future::poll(Pin::new(pending_to_endpoint), cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(()) => self.pending_to_endpoint = None,
                }
            }

            let now = Instant::now();

            // Poll the connection for packets to send on the UDP socket and try to send them on
            // `to_endpoint`.
            // FIXME max_datagrams
            if let Some(transmit) = self.connection.poll_transmit(now, 1) {
                let endpoint = self.endpoint.clone();
                debug_assert!(self.pending_to_endpoint.is_none());
                self.pending_to_endpoint = Some(Box::pin(async move {
                    // TODO: ECN bits not handled
                    endpoint
                        .send_udp_packet(transmit.destination, transmit.contents)
                        .await;
                }));
                continue 'send_pending;
            }

            // Timeout system.
            // We break out of the following loop until if `poll_timeout()` returns `None` or if
            // polling `self.next_timeout` returns `Poll::Pending`.
            loop {
                if let Some(next_timeout) = &mut self.next_timeout {
                    match Future::poll(Pin::new(next_timeout), cx) {
                        Poll::Ready(when) => {
                            self.connection.handle_timeout(when);
                            self.next_timeout = None;
                        }
                        Poll::Pending => break,
                    }
                } else if let Some(when) = self.connection.poll_timeout() {
                    if when <= now {
                        self.connection.handle_timeout(now);
                    } else {
                        //let delay = when - now;
                        self.next_timeout = Some(Timer::at(when));
                    }
                } else {
                    break;
                }
            }

            // The connection also needs to be able to send control messages to the endpoint. This is
            // handled here, and we try to send them on `to_endpoint` as well.
            if let Some(endpoint_event) = self.connection.poll_endpoint_events() {
                let endpoint = self.endpoint.clone();
                let connection_id = self.connection_id;
                debug_assert!(self.pending_to_endpoint.is_none());
                self.pending_to_endpoint = Some(Box::pin(async move {
                    endpoint
                        .report_quinn_event(connection_id, endpoint_event)
                        .await;
                }));
                continue 'send_pending;
            }

            // The final step consists in handling the events related to the various substreams.
            while let Some(event) = self.connection.poll() {
                match event {
                    quinn_proto::Event::Stream(quinn_proto::StreamEvent::Opened {
                        dir: quinn_proto::Dir::Uni,
                    })
                    | quinn_proto::Event::Stream(quinn_proto::StreamEvent::Available {
                        dir: quinn_proto::Dir::Uni,
                    })
                    | quinn_proto::Event::DatagramReceived => {
                        // We don't use datagrams or unidirectional streams. If these events
                        // happen, it is by some code not compatible with libp2p-quic.
                        self.connection
                            .close(Instant::now(), From::from(0u32), Default::default());
                    }
                    quinn_proto::Event::Stream(quinn_proto::StreamEvent::Readable { id }) => {
                        return Poll::Ready(ConnectionEvent::StreamReadable(id));
                    }
                    quinn_proto::Event::Stream(quinn_proto::StreamEvent::Writable { id }) => {
                        return Poll::Ready(ConnectionEvent::StreamWritable(id));
                    }
                    quinn_proto::Event::Stream(quinn_proto::StreamEvent::Stopped {
                        id, ..
                    }) => {
                        // The `Stop` QUIC event is more or less similar to a `Reset`, except that
                        // it applies only on the writing side of the pipe.
                        return Poll::Ready(ConnectionEvent::StreamStopped(id));
                    }
                    quinn_proto::Event::Stream(quinn_proto::StreamEvent::Available {
                        dir: quinn_proto::Dir::Bi,
                    }) => {
                        return Poll::Ready(ConnectionEvent::StreamAvailable);
                    }
                    quinn_proto::Event::Stream(quinn_proto::StreamEvent::Opened {
                        dir: quinn_proto::Dir::Bi,
                    }) => {
                        return Poll::Ready(ConnectionEvent::StreamOpened);
                    }
                    quinn_proto::Event::ConnectionLost { reason } => {
                        debug_assert!(self.closed.is_none());
                        self.is_handshaking = false;
                        let err = Error::Quinn(reason);
                        self.closed = Some(err.clone());
                        // self.close();
                        // self.connection
                        //    .close(Instant::now(), From::from(0u32), Default::default());
                        return Poll::Ready(ConnectionEvent::ConnectionLost(err));
                    }
                    quinn_proto::Event::Stream(quinn_proto::StreamEvent::Finished { id }) => {
                        return Poll::Ready(ConnectionEvent::StreamFinished(id));
                    }
                    quinn_proto::Event::Connected => {
                        debug_assert!(self.is_handshaking);
                        debug_assert!(!self.connection.is_handshaking());
                        self.is_handshaking = false;
                        return Poll::Ready(ConnectionEvent::Connected);
                    }
                    quinn_proto::Event::HandshakeDataReady => {
                        if !self.is_handshaking {
                            tracing::error!("Got HandshakeDataReady while not handshaking");
                        }
                    }
                }
            }

            break;
        }

        Poll::Pending
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Connection").finish()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let is_drained = self.connection.is_drained();
        if !is_drained {
            self.close();
        }
    }
}

/// Event generated by the [`Connection`].
#[derive(Debug)]
pub(crate) enum ConnectionEvent {
    /// Now connected to the remote and certificates are available.
    Connected,

    /// Connection has been closed and can no longer be used.
    ConnectionLost(Error),

    /// Generated after [`Connection::pop_incoming_substream`] has been called and has returned
    /// `None`. After this event has been generated, this method is guaranteed to return `Some`.
    StreamAvailable,
    /// Generated after [`Connection::pop_outgoing_substream`] has been called and has returned
    /// `None`. After this event has been generated, this method is guaranteed to return `Some`.
    StreamOpened,

    /// Generated after [`Connection::read_substream`] has been called and has returned a
    /// `Blocked` error.
    StreamReadable(quinn_proto::StreamId),
    /// Generated after [`Connection::write_substream`] has been called and has returned a
    /// `Blocked` error.
    StreamWritable(quinn_proto::StreamId),

    /// Generated after [`Connection::shutdown_substream`] has been called.
    StreamFinished(quinn_proto::StreamId),
    /// A substream has been stopped. This concept is similar to the concept of a substream being
    /// "reset", as in a TCP socket being reset for example.
    StreamStopped(quinn_proto::StreamId),
}
