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

use crate::{
    provider::Provider,
    transport::{ProtocolVersion, SocketFamily},
    ConnectError, Connection, Error,
};

use bytes::BytesMut;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use quinn_proto::VarInt;
use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::ControlFlow,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

// The `Driver` drops packets if the channel to the connection
// or transport is full.
// Set capacity 10 to avoid unnecessary packet drops if the receiver
// is only very briefly busy, but not buffer a large amount of packets
// if it is blocked longer.
const CHANNEL_CAPACITY: usize = 10;

/// Config for the transport.
#[derive(Clone)]
pub struct Config {
    /// Timeout for the initial handshake when establishing a connection.
    /// The actual timeout is the minimum of this an the [`Config::max_idle_timeout`].
    pub handshake_timeout: Duration,
    /// Maximum duration of inactivity in ms to accept before timing out the connection.
    pub max_idle_timeout: u32,
    /// Period of inactivity before sending a keep-alive packet.
    /// Must be set lower than the idle_timeout of both
    /// peers to be effective.
    ///
    /// See [`quinn_proto::TransportConfig::keep_alive_interval`] for more
    /// info.
    pub keep_alive_interval: Duration,
    /// Maximum number of incoming bidirectional streams that may be open
    /// concurrently by the remote peer.
    pub max_concurrent_stream_limit: u32,

    /// Max unacknowledged data in bytes that may be send on a single stream.
    pub max_stream_data: u32,

    /// Max unacknowledged data in bytes that may be send in total on all streams
    /// of a connection.
    pub max_connection_data: u32,

    /// Support QUIC version draft-29 for dialing and listening.
    ///
    /// Per default only QUIC Version 1 / [`libp2p_core::multiaddr::Protocol::QuicV1`]
    /// is supported.
    ///
    /// If support for draft-29 is enabled servers support draft-29 and version 1 on all
    /// QUIC listening addresses.
    /// As client the version is chosen based on the remote's address.
    pub support_draft_29: bool,

    /// TLS client config for the inner [`quinn_proto::ClientConfig`].
    client_tls_config: Arc<rustls::ClientConfig>,
    /// TLS server config for the inner [`quinn_proto::ServerConfig`].
    server_tls_config: Arc<rustls::ServerConfig>,
}

impl Config {
    /// Creates a new configuration object with default values.
    pub fn new(keypair: &libp2p_identity::Keypair) -> Self {
        let client_tls_config = Arc::new(libp2p_tls::make_client_config(keypair, None).unwrap());
        let server_tls_config = Arc::new(libp2p_tls::make_server_config(keypair).unwrap());
        Self {
            client_tls_config,
            server_tls_config,
            support_draft_29: false,
            handshake_timeout: Duration::from_secs(5),
            max_idle_timeout: 30 * 1000,
            max_concurrent_stream_limit: 256,
            keep_alive_interval: Duration::from_secs(15),
            max_connection_data: 15_000_000,

            // Ensure that one stream is not consuming the whole connection.
            max_stream_data: 10_000_000,
        }
    }
}

/// Represents the inner configuration for [`quinn_proto`].
#[derive(Debug, Clone)]
pub struct QuinnConfig {
    client_config: quinn_proto::ClientConfig,
    server_config: Arc<quinn_proto::ServerConfig>,
    endpoint_config: Arc<quinn_proto::EndpointConfig>,
}

impl From<Config> for QuinnConfig {
    fn from(config: Config) -> QuinnConfig {
        let Config {
            client_tls_config,
            server_tls_config,
            max_idle_timeout,
            max_concurrent_stream_limit,
            keep_alive_interval,
            max_connection_data,
            max_stream_data,
            support_draft_29,
            handshake_timeout: _,
        } = config;
        let mut transport = quinn_proto::TransportConfig::default();
        // Disable uni-directional streams.
        transport.max_concurrent_uni_streams(0u32.into());
        transport.max_concurrent_bidi_streams(max_concurrent_stream_limit.into());
        // Disable datagrams.
        transport.datagram_receive_buffer_size(None);
        transport.keep_alive_interval(Some(keep_alive_interval));
        transport.max_idle_timeout(Some(VarInt::from_u32(max_idle_timeout).into()));
        transport.allow_spin(false);
        transport.stream_receive_window(max_stream_data.into());
        transport.receive_window(max_connection_data.into());
        let transport = Arc::new(transport);

        let mut server_config = quinn_proto::ServerConfig::with_crypto(server_tls_config);
        server_config.transport = Arc::clone(&transport);
        // Disables connection migration.
        // Long-term this should be enabled, however we then need to handle address change
        // on connections in the `Connection`.
        server_config.migration(false);

        let mut client_config = quinn_proto::ClientConfig::new(client_tls_config);
        client_config.transport_config(transport);

        let mut endpoint_config = quinn_proto::EndpointConfig::default();
        if !support_draft_29 {
            endpoint_config.supported_versions(vec![1]);
        }

        QuinnConfig {
            client_config,
            server_config: Arc::new(server_config),
            endpoint_config: Arc::new(endpoint_config),
        }
    }
}

/// Channel used to send commands to the [`Driver`].
#[derive(Debug, Clone)]
pub struct Channel {
    /// Channel to the background of the endpoint.
    to_endpoint: mpsc::Sender<ToEndpoint>,
    /// Address that the socket is bound to.
    /// Note: this may be a wildcard ip address.
    socket_addr: SocketAddr,
}

impl Channel {
    /// Builds a new endpoint that is listening on the [`SocketAddr`].
    pub fn new_bidirectional<P: Provider>(
        quinn_config: QuinnConfig,
        socket_addr: SocketAddr,
    ) -> Result<(Self, mpsc::Receiver<Connection>), Error> {
        // Channel for forwarding new inbound connections to the listener.
        let (new_connections_tx, new_connections_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let endpoint = Self::new::<P>(quinn_config, socket_addr, Some(new_connections_tx))?;
        Ok((endpoint, new_connections_rx))
    }

    /// Builds a new endpoint that only supports outbound connections.
    pub fn new_dialer<P: Provider>(
        quinn_config: QuinnConfig,
        socket_family: SocketFamily,
    ) -> Result<Self, Error> {
        let socket_addr = match socket_family {
            SocketFamily::Ipv4 => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
            SocketFamily::Ipv6 => SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0),
        };
        Self::new::<P>(quinn_config, socket_addr, None)
    }

    /// Spawn a new [`Driver`] that runs in the background.
    fn new<P: Provider>(
        quinn_config: QuinnConfig,
        socket_addr: SocketAddr,
        new_connections: Option<mpsc::Sender<Connection>>,
    ) -> Result<Self, Error> {
        let socket = std::net::UdpSocket::bind(socket_addr)?;
        // NOT blocking, as per man:bind(2), as we pass an IP address.
        socket.set_nonblocking(true)?;
        // Capacity 0 to back-pressure the rest of the application if
        // the udp socket is busy.
        let (to_endpoint_tx, to_endpoint_rx) = mpsc::channel(0);

        let channel = Self {
            to_endpoint: to_endpoint_tx,
            socket_addr: socket.local_addr()?,
        };

        let server_config = new_connections
            .is_some()
            .then_some(quinn_config.server_config);

        let provider_socket = P::from_socket(socket)?;

        let driver = Driver::<P>::new(
            quinn_config.endpoint_config,
            quinn_config.client_config,
            new_connections,
            server_config,
            channel.clone(),
            provider_socket,
            to_endpoint_rx,
        );

        // Drive the endpoint future in the background.
        P::spawn(driver);

        Ok(channel)
    }

    pub fn socket_addr(&self) -> &SocketAddr {
        &self.socket_addr
    }

    /// Try to send a message to the background task without blocking.
    ///
    /// This first polls the channel for capacity.
    /// If the channel is full, the message is returned in `Ok(Err(_))`
    /// and the context's waker is registered for wake-up.
    ///
    /// If the background task crashed `Err` is returned.
    pub fn try_send(
        &mut self,
        to_endpoint: ToEndpoint,
        cx: &mut Context<'_>,
    ) -> Result<Result<(), ToEndpoint>, Disconnected> {
        match self.to_endpoint.poll_ready_unpin(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => {
                debug_assert!(
                    e.is_disconnected(),
                    "mpsc::Sender can only be disconnected when calling `poll_ready_unpin"
                );

                return Err(Disconnected {});
            }
            Poll::Pending => return Ok(Err(to_endpoint)),
        };

        if let Err(e) = self.to_endpoint.start_send(to_endpoint) {
            debug_assert!(e.is_disconnected(), "We called `Sink::poll_ready` so we are guaranteed to have a slot. If this fails, it means we are disconnected.");

            return Err(Disconnected {});
        }

        Ok(Ok(()))
    }

    /// Send a message to inform the [`Driver`] about an
    /// event caused by the owner of this [`Channel`] dropping.
    /// This clones the sender to the endpoint to guarantee delivery.
    /// This should *not* be called for regular messages.
    pub fn send_on_drop(&mut self, to_endpoint: ToEndpoint) {
        let _ = self.to_endpoint.clone().try_send(to_endpoint);
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
#[error("Background task disconnected")]
pub struct Disconnected {}

/// Message sent to the endpoint background task.
#[derive(Debug)]
pub enum ToEndpoint {
    /// Instruct the [`quinn_proto::Endpoint`] to start connecting to the given address.
    Dial {
        /// UDP address to connect to.
        addr: SocketAddr,
        /// Version to dial the remote on.
        version: ProtocolVersion,
        /// Channel to return the result of the dialing to.
        result: oneshot::Sender<Result<Connection, Error>>,
    },
    /// Send by a [`quinn_proto::Connection`] when the endpoint needs to process an event generated
    /// by a connection. The event itself is opaque to us. Only `quinn_proto` knows what is in
    /// there.
    ProcessConnectionEvent {
        connection_id: quinn_proto::ConnectionHandle,
        event: quinn_proto::EndpointEvent,
    },
    /// Instruct the endpoint to send a packet of data on its UDP socket.
    SendUdpPacket(quinn_proto::Transmit),
    /// The [`GenTransport`][crate::GenTransport] dialer or listener coupled to this endpoint
    /// was dropped.
    /// Once all pending connections are closed, the [`Driver`] should shut down.
    Decoupled,
}

/// Driver that runs in the background for as long as the endpoint is alive. Responsible for
/// processing messages and the UDP socket.
///
/// # Behaviour
///
/// This background task is responsible for the following:
///
/// - Sending packets on the UDP socket.
/// - Receiving packets from the UDP socket and feed them to the [`quinn_proto::Endpoint`] state
///   machine.
/// - Transmitting events generated by the [`quinn_proto::Endpoint`] to the corresponding
///   [`crate::Connection`].
/// - Receiving messages from the `rx` and processing the requested actions. This includes
///   UDP packets to send and events emitted by the [`crate::Connection`] objects.
/// - Sending new connections on `new_connection_tx`.
///
/// When it comes to channels, there exists three main multi-producer-single-consumer channels
/// in play:
///
/// - One channel, represented by `EndpointChannel::to_endpoint` and `Driver::rx`,
///   that communicates messages from [`Channel`] to the [`Driver`].
/// - One channel for each existing connection that communicates messages from the
///   [`Driver` to that [`crate::Connection`].
/// - One channel for the [`Driver`] to send newly-opened connections to. The receiving
///   side is processed by the [`GenTransport`][crate::GenTransport].
///
///
/// ## Back-pressure
///
/// ### If writing to the UDP socket is blocked
///
/// In order to avoid an unbounded buffering of events, we prioritize sending data on the UDP
/// socket over everything else. Messages from the rest of the application sent through the
/// [`Channel`] are only processed if the UDP socket is ready so that we propagate back-pressure
/// in case of a busy socket. For connections, thus this eventually also back-pressures the
/// `AsyncWrite`on substreams.
///
///
/// ### Back-pressuring the remote if the application is busy
///
/// If the channel to a connection is full because the connection is busy, inbound datagrams
/// for that connection are dropped so that the remote is backpressured.
/// The same applies for new connections if the transport is too busy to received it.
///
///
/// # Shutdown
///
/// The background task shuts down if an [`ToEndpoint::Decoupled`] event was received and the
/// last active connection has drained.
#[derive(Debug)]
pub struct Driver<P: Provider> {
    // The actual QUIC state machine.
    endpoint: quinn_proto::Endpoint,
    // QuinnConfig for client connections.
    client_config: quinn_proto::ClientConfig,
    // Copy of the channel to the endpoint driver that is passed to each new connection.
    channel: Channel,
    // Channel to receive messages from the transport or connections.
    rx: mpsc::Receiver<ToEndpoint>,

    // Socket for sending and receiving datagrams.
    provider_socket: P,
    // Future for writing the next packet to the socket.
    next_packet_out: Option<quinn_proto::Transmit>,

    // List of all active connections, with a sender to notify them of events.
    alive_connections:
        HashMap<quinn_proto::ConnectionHandle, mpsc::Sender<quinn_proto::ConnectionEvent>>,
    // Channel to forward new inbound connections to the transport.
    // `None` if server capabilities are disabled, i.e. the endpoint is only used for dialing.
    new_connection_tx: Option<mpsc::Sender<Connection>>,
    // Whether the transport dropped its handle for this endpoint.
    is_decoupled: bool,
}

impl<P: Provider> Driver<P> {
    fn new(
        endpoint_config: Arc<quinn_proto::EndpointConfig>,
        client_config: quinn_proto::ClientConfig,
        new_connection_tx: Option<mpsc::Sender<Connection>>,
        server_config: Option<Arc<quinn_proto::ServerConfig>>,
        channel: Channel,
        socket: P,
        rx: mpsc::Receiver<ToEndpoint>,
    ) -> Self {
        Driver {
            endpoint: quinn_proto::Endpoint::new(endpoint_config, server_config),
            client_config,
            channel,
            rx,
            provider_socket: socket,
            next_packet_out: None,
            alive_connections: HashMap::new(),
            new_connection_tx,
            is_decoupled: false,
        }
    }

    /// Handle a message sent from either the [`GenTransport`](super::GenTransport)
    /// or a [`crate::Connection`].
    fn handle_message(
        &mut self,
        to_endpoint: ToEndpoint,
    ) -> ControlFlow<(), Option<quinn_proto::Transmit>> {
        match to_endpoint {
            ToEndpoint::Dial {
                addr,
                result,
                version,
            } => {
                let mut config = self.client_config.clone();
                if version == ProtocolVersion::Draft29 {
                    config.version(0xff00_001d);
                }
                // This `"l"` seems necessary because an empty string is an invalid domain
                // name. While we don't use domain names, the underlying rustls library
                // is based upon the assumption that we do.
                let (connection_id, connection) = match self.endpoint.connect(config, addr, "l") {
                    Ok(c) => c,
                    Err(err) => {
                        let _ = result.send(Err(ConnectError::from(err).into()));
                        return ControlFlow::Continue(None);
                    }
                };

                debug_assert_eq!(connection.side(), quinn_proto::Side::Client);
                let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
                let connection = Connection::from_quinn_connection(
                    self.channel.clone(),
                    connection,
                    connection_id,
                    rx,
                );
                self.alive_connections.insert(connection_id, tx);
                let _ = result.send(Ok(connection));
            }

            // A connection wants to notify the endpoint of something.
            ToEndpoint::ProcessConnectionEvent {
                connection_id,
                event,
            } => {
                let has_key = self.alive_connections.contains_key(&connection_id);
                if !has_key {
                    return ControlFlow::Continue(None);
                }
                // We "drained" event indicates that the connection no longer exists and
                // its ID can be reclaimed.
                let is_drained_event = event.is_drained();
                if is_drained_event {
                    self.alive_connections.remove(&connection_id);
                    if self.is_decoupled && self.alive_connections.is_empty() {
                        log::info!(
                            "Driver is decoupled and no active connections remain. Shutting down."
                        );
                        return ControlFlow::Break(());
                    }
                }

                let event_back = self.endpoint.handle_event(connection_id, event);

                if let Some(event_back) = event_back {
                    debug_assert!(!is_drained_event);
                    if let Some(sender) = self.alive_connections.get_mut(&connection_id) {
                        // We clone the sender to guarantee that there will be at least one
                        // free slot to send the event.
                        // The channel can not grow out of bound because an `event_back` is
                        // only sent if we previously received an event from the same connection.
                        // If the connection is busy, it won't sent us any more events to handle.
                        let _ = sender.clone().start_send(event_back);
                    } else {
                        log::error!("State mismatch: event for closed connection");
                    }
                }
            }

            // Data needs to be sent on the UDP socket.
            ToEndpoint::SendUdpPacket(transmit) => return ControlFlow::Continue(Some(transmit)),
            ToEndpoint::Decoupled => self.handle_decoupling()?,
        }
        ControlFlow::Continue(None)
    }

    /// Handle an UDP datagram received on the socket.
    /// The datagram content was written into the `socket_recv_buffer`.
    fn handle_datagram(&mut self, packet: BytesMut, packet_src: SocketAddr) -> ControlFlow<()> {
        let local_ip = self.channel.socket_addr.ip();
        // TODO: ECN bits aren't handled
        let (connec_id, event) =
            match self
                .endpoint
                .handle(Instant::now(), packet_src, Some(local_ip), None, packet)
            {
                Some(event) => event,
                None => return ControlFlow::Continue(()),
            };
        match event {
            quinn_proto::DatagramEvent::ConnectionEvent(event) => {
                // `event` has type `quinn_proto::ConnectionEvent`, which has multiple
                // variants. `quinn_proto::Endpoint::handle` however only ever returns
                // `ConnectionEvent::Datagram`.
                debug_assert!(format!("{event:?}").contains("Datagram"));

                // Redirect the datagram to its connection.
                if let Some(sender) = self.alive_connections.get_mut(&connec_id) {
                    match sender.try_send(event) {
                        Ok(()) => {}
                        Err(err) if err.is_disconnected() => {
                            // Connection was dropped by the user.
                            // Inform the endpoint that this connection is drained.
                            self.endpoint
                                .handle_event(connec_id, quinn_proto::EndpointEvent::drained());
                            self.alive_connections.remove(&connec_id);
                        }
                        Err(err) if err.is_full() => {
                            // Connection is too busy. Drop the datagram to back-pressure the remote.
                            log::debug!(
                                "Dropping packet for connection {:?} because the connection's channel is full.",
                                connec_id
                            );
                        }
                        Err(_) => unreachable!("Error is either `Full` or `Disconnected`."),
                    }
                } else {
                    log::error!("State mismatch: event for closed connection");
                }
            }
            quinn_proto::DatagramEvent::NewConnection(connec) => {
                // A new connection has been received. `connec_id` is a newly-allocated
                // identifier.
                debug_assert_eq!(connec.side(), quinn_proto::Side::Server);
                let connection_tx = match self.new_connection_tx.as_mut() {
                    Some(tx) => tx,
                    None => {
                        debug_assert!(false, "Endpoint reported a new connection even though server capabilities are disabled.");
                        return ControlFlow::Continue(());
                    }
                };

                let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
                let connection =
                    Connection::from_quinn_connection(self.channel.clone(), connec, connec_id, rx);
                match connection_tx.start_send(connection) {
                    Ok(()) => {
                        self.alive_connections.insert(connec_id, tx);
                    }
                    Err(e) if e.is_disconnected() => self.handle_decoupling()?,
                    Err(e) if e.is_full() => log::warn!(
                        "Dropping new incoming connection {:?} because the channel to the listener is full",
                        connec_id
                    ),
                    Err(_) => unreachable!("Error is either `Full` or `Disconnected`."),
                }
            }
        }
        ControlFlow::Continue(())
    }

    /// The transport dropped the channel to this [`Driver`].
    fn handle_decoupling(&mut self) -> ControlFlow<()> {
        if self.alive_connections.is_empty() {
            return ControlFlow::Break(());
        }
        // Listener was closed.
        self.endpoint.reject_new_connections();
        self.new_connection_tx = None;
        self.is_decoupled = true;
        ControlFlow::Continue(())
    }
}

/// Future that runs until the [`Driver`] is decoupled and not active connections
/// remain
impl<P: Provider> Future for Driver<P> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            // Flush any pending pocket so that the socket is reading to write an next
            // packet.
            match self.provider_socket.poll_send_flush(cx) {
                // The pending packet was send or no packet was pending.
                Poll::Ready(Ok(_)) => {
                    // Start sending a packet on the socket.
                    if let Some(transmit) = self.next_packet_out.take() {
                        self.provider_socket
                            .start_send(transmit.contents, transmit.destination);
                        continue;
                    }

                    // The endpoint might request packets to be sent out. This is handled in
                    // priority to avoid buffering up packets.
                    if let Some(transmit) = self.endpoint.poll_transmit() {
                        self.next_packet_out = Some(transmit);
                        continue;
                    }

                    // Handle messages from transport and connections.
                    match self.rx.poll_next_unpin(cx) {
                        Poll::Ready(Some(to_endpoint)) => match self.handle_message(to_endpoint) {
                            ControlFlow::Continue(Some(transmit)) => {
                                self.next_packet_out = Some(transmit);
                                continue;
                            }
                            ControlFlow::Continue(None) => continue,
                            ControlFlow::Break(()) => break,
                        },
                        Poll::Ready(None) => {
                            unreachable!("Sender side is never dropped or closed.")
                        }
                        Poll::Pending => {}
                    }
                }
                // Errors on the socket are expected to never happen, and we handle them by simply
                // printing a log message. The packet gets discarded in case of error, but we are
                // robust to packet losses and it is consequently not a logic error to proceed with
                // normal operations.
                Poll::Ready(Err(err)) => {
                    log::warn!("Error while sending on QUIC UDP socket: {:?}", err);
                    continue;
                }
                Poll::Pending => {}
            }

            // Poll for new packets from the remote.
            match self.provider_socket.poll_recv_from(cx) {
                Poll::Ready(Ok((bytes, packet_src))) => {
                    let bytes_mut = bytes.as_slice().into();
                    match self.handle_datagram(bytes_mut, packet_src) {
                        ControlFlow::Continue(()) => continue,
                        ControlFlow::Break(()) => break,
                    }
                }
                // Errors on the socket are expected to never happen, and we handle them by
                // simply printing a log message.
                Poll::Ready(Err(err)) => {
                    log::warn!("Error while receive on QUIC UDP socket: {:?}", err);
                    continue;
                }
                Poll::Pending => {}
            }

            return Poll::Pending;
        }

        Poll::Ready(())
    }
}
