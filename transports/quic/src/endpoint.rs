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

//! Background task dedicated to manage the QUIC state machine.
//!
//! Considering that all QUIC communications happen over a single UDP socket, one needs to
//! maintain a unique synchronization point that holds the state of all the active connections.
//!
//! The endpoint represents this synchronization point. It is maintained in a background task
//! whose role is to interface with the UDP socket. Communication between the background task and
//! the rest of the code only happens through channels. See the documentation of the
//! [`EndpointDriver`] for a thorough description.

use crate::{
    connection::Connection,
    tls,
    transport::{self, Provider},
};

use bytes::BytesMut;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
    ready,
};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::ControlFlow,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};
use x509_parser::nom::AsBytes;

/// Represents the configuration for the QUIC endpoint.
#[derive(Debug, Clone)]
pub struct Config {
    /// The client configuration to pass to `quinn_proto`.
    client_config: quinn_proto::ClientConfig,
    /// The server configuration to pass to `quinn_proto`.
    server_config: Arc<quinn_proto::ServerConfig>,
    /// The endpoint configuration to pass to `quinn_proto`.
    endpoint_config: Arc<quinn_proto::EndpointConfig>,
}

impl Config {
    /// Creates a new configuration object with default values.
    pub fn new(keypair: &libp2p_core::identity::Keypair) -> Result<Self, tls::ConfigError> {
        let mut transport = quinn_proto::TransportConfig::default();
        transport.max_concurrent_uni_streams(0u32.into()); // Can only panic if value is out of range.
        transport.datagram_receive_buffer_size(None);
        transport.keep_alive_interval(Some(Duration::from_millis(10)));
        let transport = Arc::new(transport);

        let client_tls_config = tls::make_client_config(keypair).unwrap();
        let server_tls_config = tls::make_server_config(keypair).unwrap();

        let mut server_config = quinn_proto::ServerConfig::with_crypto(Arc::new(server_tls_config));
        server_config.transport = Arc::clone(&transport);
        // Disables connection migration.
        // Long-term this should be enabled, however we then need to handle address change
        // on connections in the `QuicMuxer`.
        server_config.migration(false);

        let mut client_config = quinn_proto::ClientConfig::new(Arc::new(client_tls_config));
        client_config.transport = transport;
        Ok(Self {
            client_config,
            server_config: Arc::new(server_config),
            endpoint_config: Default::default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct EndpointChannel {
    /// Channel to the background of the endpoint.
    to_endpoint: mpsc::Sender<ToEndpoint>,
    /// Address that the socket is bound to.
    /// Note: this may be a wildcard ip address.
    socket_addr: SocketAddr,
}

impl EndpointChannel {
    /// Builds a new endpoint that is listening on the [`SocketAddr`].
    pub fn new_bidirectional<P: Provider>(
        config: Config,
        socket_addr: SocketAddr,
    ) -> Result<(EndpointChannel, mpsc::Receiver<Connection>), transport::TransportError> {
        let (new_connections_tx, new_connections_rx) = mpsc::channel(1);
        let endpoint = Self::new::<P>(config, socket_addr, Some(new_connections_tx))?;
        Ok((endpoint, new_connections_rx))
    }

    /// Builds a new endpoint that only supports outbound connections.
    pub fn new_dialer<P: Provider>(
        config: Config,
        is_ipv6: bool,
    ) -> Result<EndpointChannel, transport::TransportError> {
        let socket_addr = if is_ipv6 {
            SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0)
        } else {
            SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)
        };
        Self::new::<P>(config, socket_addr, None)
    }

    fn new<P: Provider>(
        config: Config,
        socket_addr: SocketAddr,
        new_connections: Option<mpsc::Sender<Connection>>,
    ) -> Result<EndpointChannel, transport::TransportError> {
        // NOT blocking, as per man:bind(2), as we pass an IP address.
        let socket = std::net::UdpSocket::bind(&socket_addr)?;
        socket.set_nonblocking(true)?;
        let (to_endpoint_tx, to_endpoint_rx) = mpsc::channel(32);

        let channel = EndpointChannel {
            to_endpoint: to_endpoint_tx,
            socket_addr: socket.local_addr()?,
        };

        let server_config = new_connections.is_some().then_some(config.server_config);
        let socket = P::from_socket(socket)?;

        let driver = EndpointDriver::<P>::new(
            config.endpoint_config,
            config.client_config,
            new_connections,
            server_config,
            channel.clone(),
            socket,
            to_endpoint_rx,
        );

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
    ) -> Result<Result<(), ToEndpoint>, mpsc::SendError> {
        match self.to_endpoint.poll_ready_unpin(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(err)) => return Err(err),
            Poll::Pending => return Ok(Err(to_endpoint)),
        };
        self.to_endpoint.start_send(to_endpoint).map(Ok)
    }

    /// Send a message to inform the [`EndpointDriver`] about an
    /// event caused by the owner of this [`EndpointChannel`] dropping.
    /// This clones the sender to the endpoint to guarantee delivery.
    /// It this should *not* be called for regular messages.
    pub fn send_on_drop(&mut self, to_endpoint: ToEndpoint) {
        let _ = self.to_endpoint.clone().try_send(to_endpoint);
    }
}

/// Message sent to the endpoint background task.
#[derive(Debug)]
pub enum ToEndpoint {
    /// Instruct the endpoint to start connecting to the given address.
    Dial {
        /// UDP address to connect to.
        addr: SocketAddr,
        /// Channel to return the result of the dialing to.
        result: oneshot::Sender<Result<Connection, quinn_proto::ConnectError>>,
    },
    /// Sent by a `quinn_proto` connection when the endpoint needs to process an event generated
    /// by a connection. The event itself is opaque to us. Only `quinn_proto` knows what is in
    /// there.
    ProcessConnectionEvent {
        connection_id: quinn_proto::ConnectionHandle,
        event: quinn_proto::EndpointEvent,
    },
    /// Instruct the endpoint to send a packet of data on its UDP socket.
    SendUdpPacket(quinn_proto::Transmit),
    /// The [`GenTransport`][transport::GenTransport] dialer or listener coupled to this endpoint 
    /// was dropped.
    /// Once all pending connection closed the [`EndpointDriver`] should shut down.
    Decoupled,
}

/// Driver that runs in the background for as long as the endpoint is alive. Responsible for
/// processing messages and the UDP socket.
///
/// The `receiver` parameter must be the receiving side of the `EndpointChannel::to_endpoint` sender.
///
/// # Behaviour
///
/// This background task is responsible for the following:
///
/// - Sending packets on the UDP socket.
/// - Receiving packets from the UDP socket and feed them to the [`quinn_proto::Endpoint`] state
///   machine.
/// - Transmitting events generated by the [`quinn_proto::Endpoint`] to the corresponding
///   [`Connection`].
/// - Receiving messages from the `receiver` and processing the requested actions. This includes
///   UDP packets to send and events emitted by the [`Connection`] objects.
/// - Sending new connections on `new_connections`.
///
/// When it comes to channels, there exists three main multi-producer-single-consumer channels
/// in play:
///
/// - One channel, represented by `EndpointChannel::to_endpoint` and `receiver`, that communicates
///   messages from [`EndpointChannel`] to the [`EndpointDriver`].
/// - One channel per each existing connection that communicates messages from the  [`EndpointDriver`]
///   to that [`Connection`].
/// - One channel for the  [`EndpointDriver`] to send newly-opened connections to. The receiving
///   side is processed by the [`GenTransport`][crate::GenTransport].
///
/// In order to avoid an unbounded buffering of events, we prioritize sending data on the UDP
/// socket over everything else. If the network interface is too busy to process our packets,
/// everything comes to a freeze (including receiving UDP packets) until it is ready to accept
/// more.
///
/// Apart from freezing when the network interface is too busy, the background task should sleep
/// as little as possible. It is in particular important for the `receiver` to be drained as
/// quickly as possible in order to avoid unnecessary back-pressure on the [`Connection`] objects.
///
/// ## Back-pressure on `new_connections`
///
/// The [`quinn_proto::Endpoint`] object contains an accept buffer, in other words a buffer of the
/// incoming connections waiting to be accepted. When a new connection is signalled, we send this
/// new connection on the `new_connection_tx` channel in an asynchronous way, and we only free a slot
/// in the accept buffer once the element has actually been enqueued on `new_connection_tx`. There
/// are therefore in total three buffers in play: the `new_connection_tx` channel itself, the queue
/// of elements being sent on `new_connection_tx`, and the accept buffer of the
/// [`quinn_proto::Endpoint`].
///
/// ## Back-pressure on connections
///
/// Because connections are processed by the user at a rate of their choice, we cannot properly
/// handle the situation where the channel from the background task to individual connections is
/// full. Sleeping the task while waiting for the connection to be processed by the user could
/// even lead to a deadlock if this processing is also sleeping waiting for some other action that
/// itself depends on the background task (e.g. if processing the connection is waiting for a
/// message arriving on a different connection).
///
/// In an ideal world, we would handle a background-task-to-connection channel being full by
/// dropping UDP packets destined to this connection, as a way to back-pressure the remote.
/// Unfortunately, the `quinn-proto` library doesn't provide any way for us to know which
/// connection a UDP packet is destined for before it has been turned into a `ConnectionEvent`,
/// and because these `ConnectionEvent`s are sometimes used to synchronize the states of the
/// endpoint and connection, it would be a logic error to silently drop them.
///
/// We handle this tricky situation by simply killing connections as soon as their associated
/// channel is full.
///
// TODO: actually implement the killing of connections if channel is full, at the moment we just
// wait
/// # Shutdown
///
/// The background task shuts down if `endpoint_weak`, `receiver` or `new_connections` become
/// disconnected/invalid. This corresponds to the lifetime of the associated [`quinn_proto::Endpoint`].
///
/// Keep in mind that we pass an `EndpointChannel` whenever we create a new connection, which
/// guarantees that the [`EndpointDriver`], is properly kept alive for as long as any QUIC
/// connection is open.
///
pub struct EndpointDriver<P: Provider> {
    // The actual QUIC state machine.
    endpoint: quinn_proto::Endpoint,
    // Config for client connections.
    client_config: quinn_proto::ClientConfig,
    // Copy of the channel to the endpoint driver that is passed to each new connection.
    channel: EndpointChannel,
    // Channel to receive messages from the transport or connections.
    rx: mpsc::Receiver<ToEndpoint>,

    // Socket for sending and receiving datagrams.
    socket: P,
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

impl<P: Provider> EndpointDriver<P> {
    fn new(
        endpoint_config: Arc<quinn_proto::EndpointConfig>,
        client_config: quinn_proto::ClientConfig,
        new_connection_tx: Option<mpsc::Sender<Connection>>,
        server_config: Option<Arc<quinn_proto::ServerConfig>>,
        channel: EndpointChannel,
        socket: P,
        rx: mpsc::Receiver<ToEndpoint>,
    ) -> Self {
        EndpointDriver {
            endpoint: quinn_proto::Endpoint::new(endpoint_config, server_config),
            client_config,
            channel,
            rx,
            socket,
            next_packet_out: None,
            alive_connections: HashMap::new(),
            new_connection_tx,
            is_decoupled: false,
        }
    }

    /// Handle a message sent from either the [`GenTransport`](super::GenTransport) or a [`Connection`].
    fn handle_message(&mut self, to_endpoint: ToEndpoint) -> ControlFlow<()> {
        match to_endpoint {
            ToEndpoint::Dial { addr, result } => {
                // This `"l"` seems necessary because an empty string is an invalid domain
                // name. While we don't use domain names, the underlying rustls library
                // is based upon the assumption that we do.
                let (connection_id, connection) =
                    match self.endpoint.connect(self.client_config.clone(), addr, "l") {
                        Ok(c) => c,
                        Err(err) => {
                            let _ = result.send(Err(err));
                            return ControlFlow::Continue(());
                        }
                    };

                debug_assert_eq!(connection.side(), quinn_proto::Side::Client);
                let (tx, rx) = mpsc::channel(16);
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
                    return ControlFlow::Continue(());
                }
                // We "drained" event indicates that the connection no longer exists and
                // its ID can be reclaimed.
                let is_drained_event = event.is_drained();
                if is_drained_event {
                    self.alive_connections.remove(&connection_id);
                    if self.is_decoupled && self.alive_connections.is_empty() {
                        log::info!(
                            "Listener closed and no active connections remain. Shutting down the background task."
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
                        // only sent if we prior received an event from the same connection.
                        // If the connection is busy, it won't sent us any more events to handle.
                        let _ = sender.clone().start_send(event_back);
                    } else {
                        log::error!("State mismatch: event for closed connection");
                    }
                }
            }

            // Data needs to be sent on the UDP socket.
            ToEndpoint::SendUdpPacket(transmit) => self.next_packet_out = Some(transmit),
            ToEndpoint::Decoupled => self.handle_decoupling()?,
        }
        ControlFlow::Continue(())
    }

    /// Handle datagram received on the socket.
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
                        log::warn!(
                            "Endpoint reported a new connection even though server capabilities are disabled."
                        );
                        return ControlFlow::Continue(());
                    }
                };

                let (tx, rx) = mpsc::channel(16);
                let connection =
                    Connection::from_quinn_connection(self.channel.clone(), connec, connec_id, rx);
                match connection_tx.try_send(connection) {
                    Ok(()) => {
                        self.alive_connections.insert(connec_id, tx);
                    }
                    Err(e) if e.is_disconnected() => self.handle_decoupling()?,
                    Err(_) => log::warn!(
                        "Dropping new incoming connection {:?} because the channel to the listener is full",
                        connec_id
                    )
                }
            }
        }
        ControlFlow::Continue(())
    }

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

impl<P: Provider> Future for EndpointDriver<P> {
    type Output = ();
    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match ready!(self.socket.poll_send_flush(cx)) {
                Ok(_) => {}
                // Errors on the socket are expected to never happen, and we handle them by simply
                // printing a log message. The packet gets discarded in case of error, but we are
                // robust to packet losses and it is consequently not a logic error to process with
                // normal operations.
                Err(err) => {
                    log::error!("Error while sending on QUIC UDP socket: {:?}", err)
                }
            }

            if let Some(transmit) = self.next_packet_out.take() {
                self.socket
                    .start_send(transmit.contents, transmit.destination);
                continue;
            }

            // The endpoint might request packets to be sent out. This is handled in priority to avoid
            // buffering up packets.
            if let Some(transmit) = self.endpoint.poll_transmit() {
                self.next_packet_out = Some(transmit);
                continue;
            }

            match self.rx.poll_next_unpin(cx) {
                Poll::Ready(Some(to_endpoint)) => match self.handle_message(to_endpoint) {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(()) => break,
                },
                Poll::Ready(None) => {
                    unreachable!("Sender side is never dropped or closed.")
                }
                Poll::Pending => {}
            }

            match self.socket.poll_recv_from(cx) {
                Poll::Ready(Ok((bytes, packet_src))) => {
                    let bytes_mut = bytes.as_bytes().into();
                    match self.handle_datagram(bytes_mut, packet_src) {
                        ControlFlow::Continue(()) => continue,
                        ControlFlow::Break(()) => break,
                    }
                }
                // Errors on the socket are expected to never happen, and we handle them by
                // simply printing a log message.
                Poll::Ready(Err(err)) => {
                    log::error!("Error while receive on QUIC UDP socket: {:?}", err);
                    continue;
                }
                Poll::Pending => {}
            }
            return Poll::Pending;
        }

        Poll::Ready(())
    }
}
