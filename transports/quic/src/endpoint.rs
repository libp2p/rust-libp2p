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
//! The [`Endpoint`] object represents this synchronization point. It maintains a background task
//! whose role is to interface with the UDP socket. Communication between the background task and
//! the rest of the code only happens through channels. See the documentation of the
//! [`background_task`] for a thorough description.

use crate::{connection::Connection, tls, transport};

use futures::{
    channel::{
        mpsc::{self, SendError},
        oneshot,
    },
    prelude::*,
};
use quinn_proto::{ClientConfig as QuinnClientConfig, ServerConfig as QuinnServerConfig};
use std::{
    collections::HashMap,
    fmt,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

/// Represents the configuration for the [`Endpoint`].
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

        let mut server_config = QuinnServerConfig::with_crypto(Arc::new(server_tls_config));
        server_config.transport = Arc::clone(&transport);
        // Disables connection migration.
        // Long-term this should be enabled, however we then need to handle address change
        // on connections in the `QuicMuxer`.
        server_config.migration(false);

        let mut client_config = QuinnClientConfig::new(Arc::new(client_tls_config));
        client_config.transport = transport;
        Ok(Self {
            client_config,
            server_config: Arc::new(server_config),
            endpoint_config: Default::default(),
        })
    }
}

/// Object containing all the QUIC resources shared between all connections.
#[derive(Clone)]
pub struct Endpoint {
    /// Channel to the background of the endpoint.
    to_endpoint: mpsc::Sender<ToEndpoint>,
    /// Address that the socket is bound to.
    /// Note: this may be a wildcard ip address.
    socket_addr: SocketAddr,
}

impl Endpoint {
    /// Builds a new [`Endpoint`] that is listening on the [`SocketAddr`].
    pub fn new_bidirectional(
        config: Config,
        socket_addr: SocketAddr,
    ) -> Result<(Endpoint, mpsc::Receiver<Connection>), transport::Error> {
        let (new_connections_tx, new_connections_rx) = mpsc::channel(1);
        let endpoint = Self::new(config, socket_addr, Some(new_connections_tx))?;
        Ok((endpoint, new_connections_rx))
    }

    /// Builds a new [`Endpoint`] that only supports outbound connections.
    pub fn new_dialer(config: Config, is_ipv6: bool) -> Result<Endpoint, transport::Error> {
        let socket_addr = if is_ipv6 {
            SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0)
        } else {
            SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)
        };
        Self::new(config, socket_addr, None)
    }

    fn new(
        config: Config,
        socket_addr: SocketAddr,
        new_connections: Option<mpsc::Sender<Connection>>,
    ) -> Result<Endpoint, transport::Error> {
        // NOT blocking, as per man:bind(2), as we pass an IP address.
        let socket = std::net::UdpSocket::bind(&socket_addr)?;
        let (to_endpoint_tx, to_endpoint_rx) = mpsc::channel(32);

        let endpoint = Endpoint {
            to_endpoint: to_endpoint_tx,
            socket_addr: socket.local_addr()?,
        };

        let server_config = new_connections.map(|c| (c, config.server_config.clone()));

        // TODO: just for testing, do proper task spawning
        async_global_executor::spawn(background_task(
            config.endpoint_config,
            config.client_config,
            server_config,
            endpoint.clone(),
            async_io::Async::<UdpSocket>::new(socket)?,
            to_endpoint_rx.fuse(),
        ))
        .detach();

        Ok(endpoint)
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
    ) -> Result<Result<(), ToEndpoint>, SendError> {
        match self.to_endpoint.poll_ready_unpin(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(err)) => return Err(err),
            Poll::Pending => return Ok(Err(to_endpoint)),
        };
        self.to_endpoint.start_send(to_endpoint).map(Ok)
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
    SendUdpPacket {
        /// Destination of the UDP packet.
        destination: SocketAddr,
        /// Packet of data to send.
        data: Vec<u8>,
    },
}

/// Task that runs in the background for as long as the endpoint is alive. Responsible for
/// processing messages and the UDP socket.
///
/// The `receiver` parameter must be the receiving side of the `Endpoint::to_endpoint` sender.
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
/// - One channel, represented by `Endpoint::to_endpoint` and `receiver`, that communicates
///   messages from [`Endpoint`] to the background task.
/// - One channel per each existing connection that communicates messages from the background
///   task to that [`Connection`].
/// - One channel for the background task to send newly-opened connections to. The receiving
///   side is processed by the [`crate::transport::Listener`].
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
/// new connection on the `new_connections` channel in an asynchronous way, and we only free a slot
/// in the accept buffer once the element has actually been enqueued on `new_connections`. There
/// are therefore in total three buffers in play: the `new_connections` channel itself, the queue
/// of elements being sent on `new_connections`, and the accept buffer of the
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
/// disconnected/invalid. This corresponds to the lifetime of the associated [`Endpoint`].
///
/// Keep in mind that we pass an `Endpoint` whenever we create a new connection, which
/// guarantees that the [`Endpoint`], and therefore the background task, is properly kept alive
/// for as long as any QUIC connection is open.
///
async fn background_task(
    endpoint_config: Arc<quinn_proto::EndpointConfig>,
    client_config: quinn_proto::ClientConfig,
    server_config: Option<(mpsc::Sender<Connection>, Arc<quinn_proto::ServerConfig>)>,
    endpoint: Endpoint,
    udp_socket: async_io::Async<UdpSocket>,
    mut receiver: stream::Fuse<mpsc::Receiver<ToEndpoint>>,
) {
    let (mut new_connections, server_config) = match server_config {
        Some((a, b)) => (Some(a), Some(b)),
        None => (None, None),
    };

    // The actual QUIC state machine.
    let mut proto_endpoint = quinn_proto::Endpoint::new(endpoint_config.clone(), server_config);

    // List of all active connections, with a sender to notify them of events.
    let mut alive_connections = HashMap::<quinn_proto::ConnectionHandle, mpsc::Sender<_>>::new();

    // Buffer where we write packets received from the UDP socket.
    let mut socket_recv_buffer = vec![0; 65536];

    // Next packet waiting to be transmitted on the UDP socket, if any.
    let mut next_packet_out: Option<(SocketAddr, Vec<u8>)> = None;

    let mut is_orphaned = false;

    // Main loop of the task.
    loop {
        // Start by flushing `next_packet_out`.
        if let Some((destination, data)) = next_packet_out.take() {
            // We block the current task until the packet is sent. This way, if the
            // network interface is too busy, we back-pressure all of our internal
            // channels.
            // TODO: set ECN bits; there is no support for them in the ecosystem right now
            match udp_socket.send_to(&data, destination).await {
                Ok(n) if n == data.len() => {}
                Ok(_) => tracing::error!(
                    "QUIC UDP socket violated expectation that packets are always fully \
                    transferred"
                ),

                // Errors on the socket are expected to never happen, and we handle them by simply
                // printing a log message. The packet gets discarded in case of error, but we are
                // robust to packet losses and it is consequently not a logic error to process with
                // normal operations.
                Err(err) => tracing::error!("Error while sending on QUIC UDP socket: {:?}", err),
            }
        }

        // The endpoint might request packets to be sent out. This is handled in priority to avoid
        // buffering up packets.
        if let Some(packet) = proto_endpoint.poll_transmit() {
            next_packet_out = Some((packet.destination, packet.contents));
            continue;
        }

        futures::select! {
            message = receiver.next() => {
                // Received a message from a different part of the code requesting us to
                // do something.
                match message {
                    None => unreachable!("Sender side is never dropped or closed."),
                    Some(ToEndpoint::Dial { addr, result }) => {
                        // This `"l"` seems necessary because an empty string is an invalid domain
                        // name. While we don't use domain names, the underlying rustls library
                        // is based upon the assumption that we do.
                        let (connection_id, connection) =
                            match proto_endpoint.connect(client_config.clone(), addr, "l") {
                                Ok(c) => c,
                                Err(err) => {
                                    let _ = result.send(Err(err));
                                    continue;
                                }
                            };

                        debug_assert_eq!(connection.side(), quinn_proto::Side::Client);
                        let (tx, rx) = mpsc::channel(16);
                        let connection = Connection::from_quinn_connection(endpoint.clone(), connection, connection_id, rx);
                        alive_connections.insert(connection_id, tx);
                        let _ = result.send(Ok(connection));
                    }

                    // A connection wants to notify the endpoint of something.
                    Some(ToEndpoint::ProcessConnectionEvent { connection_id, event }) => {
                        let has_key = alive_connections.contains_key(&connection_id);
                        if !has_key {
                            continue;
                        }
                        // We "drained" event indicates that the connection no longer exists and
                        // its ID can be reclaimed.
                        let is_drained_event = event.is_drained();
                        if is_drained_event {
                            alive_connections.remove(&connection_id);
                            if is_orphaned && alive_connections.is_empty() {
                                tracing::info!(
                                    "Listener closed and no active connections remain. Shutting down the background task."
                                );
                                return;
                            }

                        }

                        let event_back = proto_endpoint.handle_event(connection_id, event);

                        if let Some(event_back) = event_back {
                            debug_assert!(!is_drained_event);
                            if let Some(sender) = alive_connections.get_mut(&connection_id) {
                                let _ = sender.send(event_back).await; // TODO: don't await here /!\
                            } else {
                                tracing::error!("State mismatch: event for closed connection");
                            }
                        }
                    }

                    // Data needs to be sent on the UDP socket.
                    Some(ToEndpoint::SendUdpPacket { destination, data }) => {
                        debug_assert!(next_packet_out.is_none());
                        next_packet_out = Some((destination, data));
                        continue;
                    }
                }
            }
            result = udp_socket.recv_from(&mut socket_recv_buffer).fuse() => {
                let (packet_len, packet_src) = match result {
                    Ok(v) => v,
                    // Errors on the socket are expected to never happen, and we handle them by
                    // simply printing a log message.
                    Err(err) => {
                        tracing::error!("Error while receive on QUIC UDP socket: {:?}", err);
                        continue;
                    },
                };

                // Received a UDP packet from the socket.
                debug_assert!(packet_len <= socket_recv_buffer.len());
                let packet = From::from(&socket_recv_buffer[..packet_len]);
                let local_ip = udp_socket.get_ref().local_addr().ok().map(|a| a.ip());
                // TODO: ECN bits aren't handled
                let event = proto_endpoint.handle(Instant::now(), packet_src, local_ip, None, packet);

                match event {
                    None => {},
                    Some((connec_id, quinn_proto::DatagramEvent::ConnectionEvent(event))) => {
                        // Event to send to an existing connection.
                        if let Some(sender) = alive_connections.get_mut(&connec_id) {
                            let _ = sender.send(event).await; // TODO: don't await here /!\
                        } else {
                            tracing::error!("State mismatch: event for closed connection");
                        }
                    },
                    Some((connec_id, quinn_proto::DatagramEvent::NewConnection(connec))) => {
                        // A new connection has been received. `connec_id` is a newly-allocated
                        // identifier.
                        debug_assert_eq!(connec.side(), quinn_proto::Side::Server);
                        let connection_tx = match new_connections.as_mut() {
                            Some(tx) => tx,
                            None => {
                                tracing::warn!(
                                    "Endpoint reported a new connection even though server capabilities are disabled."
                                );
                                continue
                            }
                        };

                        let (tx, rx) = mpsc::channel(16);
                        let connection = Connection::from_quinn_connection(endpoint.clone(), connec, connec_id, rx);
                        match connection_tx.try_send(connection) {
                            Ok(()) => {
                                alive_connections.insert(connec_id, tx);
                            }
                            Err(e) if e.is_disconnected() => {
                                // Listener was closed.
                                proto_endpoint.reject_new_connections();
                                new_connections = None;
                                is_orphaned = true;
                                if alive_connections.is_empty() {
                                    return;
                                }
                            }
                            _ => tracing::warn!(
                                "Dropping new incoming connection because the channel to the listener is full."
                            )
                        }
                    },
                }
            }
        }
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("socket_addr", &self.socket_addr)
            .finish()
    }
}
