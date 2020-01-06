// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for QUIC/UDP/IP.
//!
//! Uses [the *tokio* library](https://tokio.rs).
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! use libp2p_quic::{QuicConfig, QuicEndpoint};
//! use libp2p_core::Multiaddr;
//!
//! # fn main() {
//! let quic_config = QuicConfig::default();
//! let quic_endpoint = QuicEndpoint::new(
//!     &quic_config,
//!     "/ip4/127.0.0.1/udp/12345/quic".parse().expect("bad address?"),
//! )
//! .expect("I/O error");
//! # }
//! ```
//!
//! The `QuicConfig` structs implements the `Transport` trait of the `swarm` library. See the
//! documentation of `swarm` and of libp2p in general to learn how to use the `Transport` trait.
//!
//! Note that QUIC provides transport, security, and multiplexing in a single protocol.  Therefore,
//! QUIC connections do not need to be upgraded.  You will get a compile-time error if you try.
//! Instead, you must pass all needed configuration into the constructor.
//!
//! # Design Notes
//!
//! The entry point is the `QuicEndpoint` struct.  It represents a single QUIC endpoint.  You
//! should generally have one of these per process.
//!
//! `QuicEndpoint` manages a background task that processes all incoming packets.  Each
//! `QuicConnection` also manages a background task, which handles socket output and timer polling.

#![forbid(
    unused_must_use,
    unstable_features,
    warnings,
    missing_copy_implementations
)]
#![deny(trivial_casts)]
mod certificate;
mod verifier;
use async_macros::ready;
use async_std::net::UdpSocket;
pub use certificate::make_cert;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    StreamMuxer, Transport,
};
use log::{debug, trace, warn};
use parking_lot::{Mutex, MutexGuard};
use quinn_proto::{Connection, ConnectionEvent, ConnectionHandle, Dir, StreamId};
use std::{
    collections::HashMap,
    io,
    mem::replace,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Weak},
    task::{
        Context,
        Poll::{self, Pending, Ready},
    },
    time::Instant,
};

/// Represents the configuration for a QUIC/UDP/IP transport capability for libp2p.
///
/// The QUIC endpoints created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone)]
pub struct QuicConfig {
    /// The client configuration.  Quinn provides functions for making one.
    pub client_config: quinn_proto::ClientConfig,
    /// The server configuration.  Quinn provides functions for making one.
    pub server_config: Arc<quinn_proto::ServerConfig>,
    /// The endpoint configuration
    pub endpoint_config: Arc<quinn_proto::EndpointConfig>,
}

fn make_client_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
) -> quinn_proto::ClientConfig {
    let mut transport = quinn_proto::TransportConfig::default();
    transport.stream_window_uni = 0;
    transport.datagram_receive_buffer_size = None;
    let mut crypto = rustls::ClientConfig::new();
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto.enable_early_data = true;
    crypto.set_single_client_cert(vec![certificate], key);
    let verifier = verifier::VeryInsecureAllowAllCertificatesWithoutChecking;
    crypto
        .dangerous()
        .set_certificate_verifier(Arc::new(verifier));
    quinn_proto::ClientConfig {
        transport: Arc::new(transport),
        crypto: Arc::new(crypto),
    }
}

fn make_server_config(
    certificate: rustls::Certificate,
    key: rustls::PrivateKey,
) -> quinn_proto::ServerConfig {
    let mut transport = quinn_proto::TransportConfig::default();
    transport.stream_window_uni = 0;
    transport.datagram_receive_buffer_size = None;
    let mut crypto = rustls::ServerConfig::new(Arc::new(
        verifier::VeryInsecureRequireClientCertificateButDoNotCheckIt,
    ));
    crypto.versions = vec![rustls::ProtocolVersion::TLSv1_3];
    crypto
        .set_single_cert(vec![certificate], key)
        .expect("we are given a valid cert; qed");
    let mut config = quinn_proto::ServerConfig::default();
    config.transport = Arc::new(transport);
    config.crypto = Arc::new(crypto);
    config
}

impl Default for QuicConfig {
    /// Creates a new configuration object for TCP/IP.
    fn default() -> Self {
        let keypair = libp2p_core::identity::Keypair::generate_ed25519();
        let cert = make_cert(&keypair);
        let (cert, key) = (
            rustls::Certificate(
                cert.serialize_der()
                    .expect("serialization of a valid cert will succeed; qed"),
            ),
            rustls::PrivateKey(cert.serialize_private_key_der()),
        );
        Self {
            client_config: make_client_config(cert.clone(), key.clone()),
            server_config: Arc::new(make_server_config(cert, key)),
            endpoint_config: Default::default(),
        }
    }
}

#[derive(Debug)]
enum EndpointMessage {
    ConnectionAccepted,
    EndpointEvent {
        handle: ConnectionHandle,
        event: quinn_proto::EndpointEvent,
    },
}

#[derive(Debug)]
struct EndpointInner {
    inner: quinn_proto::Endpoint,
    muxers: HashMap<ConnectionHandle, Weak<Mutex<Muxer>>>,
    driver: Option<async_std::task::JoinHandle<Result<(), io::Error>>>,
    /// Pending packet
    outgoing_packet: Option<quinn_proto::Transmit>,
    /// Used to receive events from connections
    event_receiver: mpsc::Receiver<EndpointMessage>,
}

impl EndpointInner {
    fn drive_receive(
        &mut self,
        socket: &UdpSocket,
        cx: &mut Context,
    ) -> Poll<Result<(ConnectionHandle, Connection), io::Error>> {
        use quinn_proto::DatagramEvent;
        let mut buf = [0; 1800];
        trace!("endpoint polling for incoming packets!");
        assert!(self.outgoing_packet.is_none());
        assert!(self.inner.poll_transmit().is_none());
        loop {
            let (bytes, peer) = ready!(socket.poll_recv_from(cx, &mut buf[..]))?;
            trace!("got a packet of length {} from {}!", bytes, peer);
            let (handle, event) =
                match self
                    .inner
                    .handle(Instant::now(), peer, None, buf[..bytes].into())
                {
                    Some(e) => e,
                    None => {
                        ready!(self.poll_transmit_pending(socket, cx))?;
                        continue;
                    }
                };
            trace!("have an event!");
            match event {
                DatagramEvent::ConnectionEvent(connection_event) => {
                    match self
                        .muxers
                        .get(&handle)
                        .expect("received a ConnectionEvent for an unknown Connection")
                        .upgrade()
                    {
                        Some(connection) => connection
                            .lock()
                            .process_connection_events(self, connection_event),
                        None => debug!("lost our connection!"),
                    }
                    ready!(self.poll_transmit_pending(socket, cx))?
                }
                DatagramEvent::NewConnection(connection) => break Ready(Ok((handle, connection))),
            }
        }
    }

    fn poll_transmit_pending(
        &mut self,
        socket: &UdpSocket,
        cx: &mut Context,
    ) -> Poll<Result<(), io::Error>> {
        if let Some(tx) = self.outgoing_packet.take() {
            ready!(self.poll_transmit(socket, cx, tx))?
        }
        while let Some(tx) = self.inner.poll_transmit() {
            ready!(self.poll_transmit(socket, cx, tx))?
        }
        Ready(Ok(()))
    }

    fn poll_transmit(
        &mut self,
        socket: &UdpSocket,
        cx: &mut Context,
        packet: quinn_proto::Transmit,
    ) -> Poll<Result<(), io::Error>> {
        match socket.poll_send_to(cx, &packet.contents, &packet.destination) {
            Pending => {
                self.outgoing_packet = Some(packet);
                return Pending;
            }
            Ready(Ok(size)) => {
                debug_assert_eq!(size, packet.contents.len());
                trace!(
                    "sent packet of length {} to {}",
                    packet.contents.len(),
                    packet.destination
                );
                Ready(Ok(()))
            }
            Ready(Err(e)) => return Ready(Err(e)),
        }
    }
}

#[derive(Debug)]
struct Endpoint {
    /// The single UDP socket used for I/O
    socket: UdpSocket,
    /// A `Mutex` protecting the QUIC state machine.
    inner: Mutex<EndpointInner>,
    /// The channel on which new connections are sent.  This is bounded in practice by the accept
    /// backlog.
    new_connections: mpsc::UnboundedSender<Result<ListenerEvent<QuicUpgrade>, io::Error>>,
    /// The channel used to receive new connections.
    receive_connections:
        Mutex<Option<mpsc::UnboundedReceiver<Result<ListenerEvent<QuicUpgrade>, io::Error>>>>,
    /// Connections send their events to this
    event_channel: mpsc::Sender<EndpointMessage>,
    /// The `Multiaddr`
    address: Multiaddr,
}

#[derive(Debug, Clone)]
pub struct QuicMuxer(Arc<Mutex<Muxer>>);

impl QuicMuxer {
    fn inner<'a>(&'a self) -> MutexGuard<'a, Muxer> {
        self.0.lock()
    }
}

#[derive(Debug)]
enum OutboundInner {
    Complete(Result<StreamId, io::Error>),
    Pending(oneshot::Receiver<StreamId>),
    Done,
}

pub struct Outbound(OutboundInner);

impl Future for Outbound {
    type Output = Result<StreamId, io::Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;
        match this.0 {
            OutboundInner::Complete(_) => match replace(&mut this.0, OutboundInner::Done) {
                OutboundInner::Complete(e) => Ready(e),
                _ => unreachable!(),
            },
            OutboundInner::Pending(ref mut receiver) => {
                let result = ready!(receiver.poll_unpin(cx))
                    .map_err(|oneshot::Canceled| io::ErrorKind::ConnectionAborted.into());
                this.0 = OutboundInner::Done;
                Ready(result)
            }
            OutboundInner::Done => panic!("polled after yielding Ready"),
        }
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = Outbound;
    type Substream = StreamId;
    type Error = io::Error;
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let mut inner = self.inner();
        if let Some(ref e) = inner.close_reason {
            Outbound(OutboundInner::Complete(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                e.clone(),
            ))))
        } else if let Some(id) = inner.pending_stream.take() {
            // mandatory ― otherwise we will fail an assertion above
            inner.wake_driver();
            Outbound(OutboundInner::Complete(Ok(id)))
        } else if let Some(id) = inner.connection.open(Dir::Bi) {
            // optimization: if we can complete synchronously, do so.
            inner.wake_driver();
            Outbound(OutboundInner::Complete(Ok(id)))
        } else {
            let (sender, receiver) = oneshot::channel();
            inner.connectors.push_front(sender);
            inner.wake_driver();
            Outbound(OutboundInner::Pending(receiver))
        }
    }
    fn destroy_outbound(&self, _: Outbound) {}
    fn destroy_substream(&self, _substream: Self::Substream) {}
    fn is_remote_acknowledged(&self) -> bool {
        true
    }

    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, Self::Error>> {
        debug!("being polled for inbound connections!");
        let mut inner = self.inner();
        if let Some(ref e) = inner.close_reason {
            return Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                e.clone(),
            )));
        }
        inner.wake_driver();
        match inner.connection.accept(quinn_proto::Dir::Bi) {
            None => {
                if let Some(waker) = replace(&mut inner.accept_waker, Some(cx.waker().clone())) {
                    waker.wake()
                }
                Pending
            }
            Some(stream) => Ready(Ok(stream)),
        }
    }

    fn write_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::WriteError;
        let mut inner = self.inner();
        inner.wake_driver();
        match inner.connection.write(*substream, buf) {
            Ok(bytes) => {
                inner.wake_driver();
                Ready(Ok(bytes))
            }
            Err(WriteError::Blocked) => {
                inner.wake_driver();
                inner.writers.insert(*substream, cx.waker().clone());
                Pending
            }
            Err(WriteError::UnknownStream) => {
                panic!("libp2p never uses a closed stream, so this cannot happen; qed")
            }
            Err(WriteError::Stopped(_)) => Ready(Err(io::ErrorKind::ConnectionAborted.into())),
        }
    }

    fn poll_outbound(
        &self,
        cx: &mut Context,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        substream.poll_unpin(cx)
    }

    fn read_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::ReadError;
        let mut inner = self.inner();
        match inner.connection.read(*substream, buf) {
            Ok(Some(bytes)) => {
                inner.wake_driver();
                Ready(Ok(bytes))
            }
            Ok(None) => Ready(Ok(0)),
            Err(ReadError::Blocked) => {
                inner.wake_driver();
                inner.readers.insert(*substream, cx.waker().clone());
                Pending
            }
            Err(ReadError::UnknownStream) => unreachable!("use of a closed stream by libp2p"),
            Err(ReadError::Reset(_)) => Ready(Err(io::ErrorKind::ConnectionReset.into())),
        }
    }

    fn shutdown_substream(
        &self,
        _cx: &mut Context,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        Ready(
            self.inner()
                .connection
                .finish(*substream)
                .map_err(|e| match e {
                    quinn_proto::FinishError::UnknownStream => {
                        panic!("libp2p never uses a closed stream, so this cannot happen; qed")
                    }
                    quinn_proto::FinishError::Stopped { .. } => {
                        io::ErrorKind::ConnectionReset.into()
                    }
                }),
        )
    }

    fn flush_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        self.write_substream(cx, substream, b"").map_ok(drop)
    }

    fn flush_all(&self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Ready(Ok(()))
    }

    fn close(&self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Ready(Ok(self.inner().connection.close(
            Instant::now(),
            Default::default(),
            Default::default(),
        )))
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct QuicStream {
    id: StreamId,
    muxer: QuicMuxer,
}

#[cfg(test)]
impl AsyncWrite for QuicStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let inner = self.get_mut();
        inner.muxer.write_substream(cx, &mut inner.id, buf)
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Result<(), io::Error>> {
        unimplemented!()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        self.poll_write(cx, b"").map_ok(drop)
    }
}

#[cfg(test)]
impl AsyncRead for QuicStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let inner = self.get_mut();
        inner.muxer.read_substream(cx, &mut inner.id, buf)
    }
}

#[cfg(test)]
impl Stream for QuicMuxer {
    type Item = QuicStream;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.poll_inbound(cx).map(|x| match x {
            Ok(id) => Some(QuicStream {
                id,
                muxer: self.get_mut().clone(),
            }),
            Err(_) => None,
        })
    }
}

/// A QUIC endpoint.  Each endpoint has its own configuration and listening socket.
///
/// You and You generally need only one of these per process.  Endpoints are thread-safe, so you
/// can share them among as many threads as you like.  However, performance may be better if you
/// have one per CPU core, as this reduces lock contention.  Most applications will not need to
/// worry about this.  `QuicEndpoint` tries to use fine-grained locking to reduce the overhead.
///
/// `QuicEndpoint` wraps the underlying data structure in an `Arc`, so cloning it just bumps the
/// reference count.  All state is shared between the clones.  For example, you can pass different
/// clones to `listen_on`.  Each incoming connection will be received by exactly one of them.
///
/// The **only** valid `Multiaddr` to pass to `listen_on` or `dial` is the one used to create the
/// `QuicEndpoint`.  You can obtain this via the `addr` method.  If you pass a different one, you
/// will get `TransportError::MultiaddrNotSuppported`.
#[derive(Debug, Clone)]
pub struct QuicEndpoint(Arc<Endpoint>, QuicConfig);

impl QuicEndpoint {
    fn inner(&self) -> MutexGuard<'_, EndpointInner> {
        self.0.inner.lock()
    }

    /// Construct a `QuicEndpoint` with the given `QuicConfig` and `Multiaddr`.
    pub fn new(
        config: &QuicConfig,
        address: Multiaddr,
    ) -> Result<Self, TransportError<<&Self as Transport>::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&address) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(address));
        };
        // NOT blocking, as per man:bind(2), as we pass an IP address.
        let socket = std::net::UdpSocket::bind(&socket_addr)?.into();
        let (new_connections, receive_connections) = mpsc::unbounded();
        let (event_channel, event_receiver) = mpsc::channel(0);
        new_connections
            .unbounded_send(Ok(ListenerEvent::NewAddress(address.clone())))
            .expect("we have a reference to the peer, so this will not fail; qed");
        Ok(Self(
            Arc::new(Endpoint {
                socket,
                inner: Mutex::new(EndpointInner {
                    inner: quinn_proto::Endpoint::new(
                        config.endpoint_config.clone(),
                        Some(config.server_config.clone()),
                    )
                    .map_err(|_| TransportError::Other(io::ErrorKind::InvalidData.into()))?,
                    muxers: HashMap::new(),
                    driver: None,
                    event_receiver,
                    outgoing_packet: None,
                }),
                address,
                receive_connections: Mutex::new(Some(receive_connections)),
                new_connections,
                event_channel,
            }),
            config.clone(),
        ))
    }

    fn create_muxer(
        &self,
        connection: Connection,
        handle: ConnectionHandle,
        inner: &mut EndpointInner,
    ) -> (QuicMuxer, ConnectionDriver) {
        let (driver, muxer) = ConnectionDriver::new(Muxer::new(self.0.clone(), connection, handle));
        inner.muxers.insert(handle, Arc::downgrade(&muxer));
        (QuicMuxer(muxer), driver)
    }

    fn accept_muxer(
        &self,
        connection: Connection,
        handle: ConnectionHandle,
        inner: &mut EndpointInner,
    ) {
        let (muxer, driver) = self.create_muxer(connection, handle, &mut *inner);
        async_std::task::spawn(driver);
        self.0
            .new_connections
            .unbounded_send(Ok(ListenerEvent::Upgrade {
                upgrade: QuicUpgrade { muxer: Some(muxer) },
                local_addr: self.0.address.clone(),
                remote_addr: self.0.address.clone(),
            }))
            .expect(
                "this is an unbounded channel, and we have an instance \
                 of the peer, so this will never fail; qed",
            );
    }
}

impl Future for QuicEndpoint {
    type Output = Result<(), io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        let mut inner = this.inner();
        'a: loop {
            ready!(inner.poll_transmit_pending(&this.0.socket, cx))?;
            while let Ready(e) = inner.event_receiver.poll_next_unpin(cx).map(|e| e.unwrap()) {
                match e {
                    EndpointMessage::ConnectionAccepted => {
                        debug!("accepting connection!");
                        inner.inner.accept();
                    }
                    EndpointMessage::EndpointEvent { handle, event } => {
                        debug!("we have an event from connection {:?}", handle);
                        match inner.muxers.get(&handle).and_then(|e| e.upgrade()) {
                            None => drop(inner.muxers.remove(&handle)),
                            Some(connection) => match inner.inner.handle_event(handle, event) {
                                None => {
                                    connection.lock().process_endpoint_communication(&mut inner)
                                }
                                Some(event) => connection
                                    .lock()
                                    .process_connection_events(&mut inner, event),
                            },
                        }
                    }
                }
                ready!(inner.poll_transmit_pending(&this.0.socket, cx))?;
            }
            let (handle, connection) = ready!(inner.drive_receive(&this.0.socket, cx))?;
            this.accept_muxer(connection, handle, &mut *inner);
            ready!(inner.poll_transmit_pending(&this.0.socket, cx))?
        }
    }
}

#[derive(Debug)]
pub struct QuicUpgrade {
    muxer: Option<QuicMuxer>,
}

impl Future for QuicUpgrade {
    type Output = Result<QuicMuxer, io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let muxer = &mut self.get_mut().muxer;
        trace!("outbound polling!");
        {
            let mut inner = muxer.as_mut().expect("polled after yielding Ready").inner();
            if let Some(ref e) = inner.close_reason {
                return Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    e.clone(),
                )));
            } else if inner.connection.is_handshaking() {
                assert!(inner.close_reason.is_none());
                assert!(!inner.connection.is_drained(), "deadlock");
                inner.accept_waker = Some(cx.waker().clone());
                return Pending;
            } else if inner.connection.is_drained() {
                debug!("connection already drained; failing");
                return Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    inner
                        .close_reason
                        .as_ref()
                        .expect("drained connections have a close reason")
                        .clone(),
                )));
            } else if inner.connection.side().is_server() {
                ready!(inner.endpoint_channel.poll_ready(cx))
                    .expect("the other side is not closed; qed");
                inner
                    .endpoint_channel
                    .start_send(EndpointMessage::ConnectionAccepted)
                    .expect("we just checked that we can send some data; qed");
            }
        }
        Ready(Ok(muxer.take().expect("impossible")))
    }
}

impl Transport for &QuicEndpoint {
    type Output = QuicMuxer;
    type Error = io::Error;
    type Listener =
        mpsc::UnboundedReceiver<Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>>;
    type ListenerUpgrade = QuicUpgrade;
    type Dial = QuicUpgrade;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        if addr != self.0.address {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        let res = (self.0)
            .receive_connections
            .lock()
            .take()
            .ok_or_else(|| TransportError::Other(io::ErrorKind::AlreadyExists.into()));
        let mut inner = self.inner();
        if inner.driver.is_none() {
            inner.driver = Some(async_std::task::spawn(self.clone()))
        }
        res
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                debug!("Instantly refusing dialing {}, as it is invalid", addr);
                return Err(TransportError::Other(
                    io::ErrorKind::ConnectionRefused.into(),
                ));
            }
            socket_addr
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        let mut inner = self.inner();
        if inner.driver.is_none() {
            inner.driver = Some(async_std::task::spawn(self.clone()))
        }

        let s: Result<(_, Connection), _> = inner
            .inner
            .connect(self.1.client_config.clone(), socket_addr, "localhost")
            .map_err(|e| {
                warn!("Connection error: {:?}", e);
                TransportError::Other(io::ErrorKind::InvalidInput.into())
            });
        let (handle, conn) = s?;
        let (muxer, driver) = self.create_muxer(conn, handle, &mut inner);
        async_std::task::spawn(driver);
        Ok(QuicUpgrade { muxer: Some(muxer) })
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;
    let proto3 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        return Err(());
    }

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::Quic) => {
            Ok(SocketAddr::new(ip.into(), port))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::Quic) => {
            Ok(SocketAddr::new(ip.into(), port))
        }
        _ => Err(()),
    }
}

type StreamSenderQueue = std::collections::VecDeque<oneshot::Sender<StreamId>>;

#[derive(Debug)]
pub struct Muxer {
    /// The pending stream, if any.
    pending_stream: Option<StreamId>,
    /// The associated endpoint
    endpoint: Arc<Endpoint>,
    /// The `quinn_proto::Connection` struct.
    connection: Connection,
    /// Connection handle
    handle: ConnectionHandle,
    /// Tasks blocked on writing
    writers: HashMap<StreamId, std::task::Waker>,
    /// Tasks blocked on reading
    readers: HashMap<StreamId, std::task::Waker>,
    /// Task waiting for new connections, or for this connection to complete.
    accept_waker: Option<std::task::Waker>,
    /// Tasks waiting to make a connection
    connectors: StreamSenderQueue,
    /// Pending transmit
    pending: Option<quinn_proto::Transmit>,
    /// The timers being used by this connection
    timers: quinn_proto::TimerTable<Option<futures_timer::Delay>>,
    /// The close reason, if this connection has been lost
    close_reason: Option<quinn_proto::ConnectionError>,
    /// Waker to wake up the driver
    waker: Option<std::task::Waker>,
    /// Channel for endpoint events
    endpoint_channel: mpsc::Sender<EndpointMessage>,
}

impl Muxer {
    fn wake_driver(&mut self) {
        if let Some(waker) = self.waker.take() {
            debug!("driver awoken!");
            waker.wake();
        }
    }

    fn drive_timers(&mut self, cx: &mut Context) -> bool {
        let mut keep_going = false;
        let now = Instant::now();
        for (timer, timer_ref) in self.timers.iter_mut() {
            if let Some(ref mut timer_future) = timer_ref {
                match timer_future.poll_unpin(cx) {
                    Pending => continue,
                    Ready(()) => {
                        keep_going = true;
                        self.connection.handle_timeout(now, timer);
                        *timer_ref = None;
                    }
                }
            }
        }
        while let Some(quinn_proto::TimerUpdate { timer, update }) = self.connection.poll_timers() {
            keep_going = true;
            use quinn_proto::TimerSetting;
            match update {
                TimerSetting::Stop => self.timers[timer] = None,
                TimerSetting::Start(instant) => {
                    if instant < now {
                        self.timers[timer] = None;
                        self.connection.handle_timeout(now, timer);
                        continue;
                    }

                    trace!("setting timer {:?} for {:?}", timer, instant - now);
                    self.timers[timer] = Some(futures_timer::Delay::new(instant - now));
                }
            }
        }
        keep_going
    }

    fn new(endpoint: Arc<Endpoint>, connection: Connection, handle: ConnectionHandle) -> Self {
        Muxer {
            pending_stream: None,
            connection,
            handle,
            writers: HashMap::new(),
            readers: HashMap::new(),
            accept_waker: None,
            connectors: Default::default(),
            endpoint: endpoint.clone(),
            pending: None,
            timers: quinn_proto::TimerTable::new(|| None),
            close_reason: None,
            waker: None,
            endpoint_channel: endpoint.event_channel.clone(),
        }
    }

    /// Process all endpoint-facing events for this connection.  This is synchronous and will not
    /// fail.
    fn send_to_endpoint(&mut self, endpoint: &mut EndpointInner) {
        while let Some(endpoint_event) = self.connection.poll_endpoint_events() {
            if let Some(connection_event) = endpoint.inner.handle_event(self.handle, endpoint_event)
            {
                self.connection.handle_event(connection_event)
            }
        }
    }

    /// Call when I/O is done by the application.  `bytes` is the number of bytes of I/O done.
    fn on_application_io(&mut self, cx: &mut Context<'_>) -> Poll<Result<bool, io::Error>> {
        let now = Instant::now();
        let mut needs_polling = false;
        while let Some(event) = self.connection.poll_endpoint_events() {
            needs_polling = true;
            self.endpoint_channel
                .start_send(EndpointMessage::EndpointEvent {
                    handle: self.handle,
                    event,
                })
                .expect(
                    "we checked in `pre_application_io` that this channel had space; \
                     that is always called first, and \
                     there is a lock preventing concurrency problems; qed",
                );
            ready!(self.endpoint_channel.poll_ready(cx))
                .expect("we have a reference to the peer; qed");
        }
        while let Some(transmit) = self.connection.poll_transmit(now) {
            ready!(self.poll_transmit(cx, transmit))?;
            needs_polling = true;
        }
        Ready(Ok(needs_polling))
    }

    fn pre_application_io(&mut self, cx: &mut Context<'_>) -> Poll<Result<bool, io::Error>> {
        if let Some(ref e) = self.close_reason {
            return Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                e.clone(),
            )));
        }
        if let Some(transmit) = self.pending.take() {
            ready!(self.poll_transmit(cx, transmit))?;
        }
        ready!(self.endpoint_channel.poll_ready(cx)).expect("we have a reference to the peer; qed");
        let mut keep_going = false;
        while let Some(transmit) = self.connection.poll_transmit(Instant::now()) {
            keep_going = true;
            ready!(self.poll_transmit(cx, transmit))?;
        }
        Ready(Ok(keep_going))
    }

    fn poll_transmit(
        &mut self,
        cx: &mut Context<'_>,
        transmit: quinn_proto::Transmit,
    ) -> Poll<Result<usize, io::Error>> {
        trace!(
            "sending packet of length {} to {}",
            transmit.contents.len(),
            transmit.destination
        );
        match self
            .endpoint
            .socket
            .poll_send_to(cx, &transmit.contents, &transmit.destination)
        {
            Pending => {
                self.pending = Some(transmit);
                Pending
            }
            r @ Ready(_) => {
                trace!(
                    "sent packet of length {} to {}",
                    transmit.contents.len(),
                    transmit.destination
                );
                r
            }
        }
    }

    /// Process application events
    fn process_connection_events(&mut self, endpoint: &mut EndpointInner, event: ConnectionEvent) {
        self.connection.handle_event(event);
        self.process_endpoint_communication(endpoint)
    }

    fn process_endpoint_communication(&mut self, endpoint: &mut EndpointInner) {
        if self.close_reason.is_some() {
            return;
        }
        self.send_to_endpoint(endpoint);
        self.process_app_events();
        self.wake_driver();
        assert!(self.connection.poll_endpoint_events().is_none());
        assert!(self.connection.poll().is_none());
        if let Some(tx) = self.connection.poll_transmit(Instant::now()) {
            assert!(replace(&mut endpoint.outgoing_packet, Some(tx)).is_none())
        }
    }

    pub fn process_app_events(&mut self) {
        use quinn_proto::Event;
        while let Some(event) = self.connection.poll() {
            match event {
                Event::StreamOpened { dir: Dir::Uni } | Event::DatagramReceived => {
                    panic!("we disabled incoming unidirectional streams and datagrams")
                }
                Event::StreamAvailable { dir: Dir::Uni } => {
                    panic!("we don’t use unidirectional streams")
                }
                Event::StreamReadable { stream } => {
                    trace!("Stream {:?} readable", stream);
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.readers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamWritable { stream } => {
                    trace!("Stream {:?} writable", stream);
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.writers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamAvailable { dir: Dir::Bi } => {
                    trace!("Bidirectional stream available");
                    if self.connectors.is_empty() {
                        // no task to wake up
                        continue;
                    }
                    debug_assert!(
                        self.pending_stream.is_none(),
                        "we cannot have both pending tasks and a pending stream; qed"
                    );
                    let stream = self.connection.open(Dir::Bi)
                            .expect("we just were told that there is a stream available; there is a mutex that prevents other threads from calling open() in the meantime; qed");
                    if let Some(oneshot) = self.connectors.pop_front() {
                        match oneshot.send(stream) {
                            Ok(()) => continue,
                            Err(_) => (),
                        }
                    }
                    self.pending_stream = Some(stream)
                }
                Event::ConnectionLost { reason } => {
                    debug!("lost connection due to {:?}", reason);
                    self.close_reason = Some(reason);
                    if let Some(w) = self.accept_waker.take() {
                        w.wake()
                    }
                    for (_, v) in self.writers.drain() {
                        v.wake();
                    }
                    for (_, v) in self.readers.drain() {
                        v.wake();
                    }
                    self.connectors.truncate(0);
                }
                Event::StreamFinished { stream, .. } => {
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.writers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                // These are separate events, but are handled the same way.
                Event::StreamOpened { dir: Dir::Bi } | Event::Connected => {
                    debug!("connected or stream opened!");
                    if let Some(w) = self.accept_waker.take() {
                        w.wake()
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
struct ConnectionDriver {
    inner: Arc<Mutex<Muxer>>,
    endpoint: Arc<Endpoint>,
    outgoing_packet: Option<quinn_proto::Transmit>,
}

impl Unpin for ConnectionDriver {}

impl ConnectionDriver {
    fn new(muxer: Muxer) -> (Self, Arc<Mutex<Muxer>>) {
        let endpoint = muxer.endpoint.clone();
        let inner = Arc::new(Mutex::new(muxer));
        (
            Self {
                inner: inner.clone(),
                endpoint,
                outgoing_packet: None,
            },
            inner,
        )
    }
}

impl Future for ConnectionDriver {
    type Output = Result<(), io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();
        debug!("being polled for timers!");
        let mut inner = this.inner.lock();
        inner.waker = Some(cx.waker().clone());
        loop {
            if inner.connection.is_drained() {
                debug!(
                    "Connection drained: close reason {}",
                    inner
                        .close_reason
                        .as_ref()
                        .expect("we never have a closed connection with no reason; qed")
                );
                break Ready(Ok(()));
            }
            let mut needs_timer_update = false;
            debug!("loop iteration");
            needs_timer_update |= ready!(inner.pre_application_io(cx))?;
            needs_timer_update |= ready!(inner.on_application_io(cx))?;
            needs_timer_update |= inner.drive_timers(cx);
            inner.process_app_events();
            if !needs_timer_update {
                break Pending;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{multiaddr_to_socketaddr, QuicConfig, QuicEndpoint};
    use futures::prelude::*;
    use libp2p_core::{
        multiaddr::{Multiaddr, Protocol},
        transport::ListenerEvent,
        Transport,
    };
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn init() {
        drop(env_logger::try_init());
    }

    #[test]
    fn wildcard_expansion() {
        init();
        let addr: Multiaddr = "/ip4/0.0.0.0/udp/1234/quic".parse().unwrap();
        let listener = QuicEndpoint::new(&QuicConfig::default(), addr.clone())
            .expect("endpoint")
            .listen_on(addr)
            .expect("listener");
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/1236/quic".parse().unwrap();
        let client = QuicEndpoint::new(&QuicConfig::default(), addr.clone())
            .expect("endpoint")
            .dial(addr)
            .expect("dialer");

        // Process all initial `NewAddress` events and make sure they
        // do not contain wildcard address or port.
        let server = listener
            .take_while(|event| match event.as_ref().unwrap() {
                ListenerEvent::NewAddress(a) => {
                    let mut iter = a.iter();
                    match iter.next().expect("ip address") {
                        Protocol::Ip4(_ip) => {} // assert!(!ip.is_unspecified()),
                        Protocol::Ip6(_ip) => {} // assert!(!ip.is_unspecified()),
                        other => panic!("Unexpected protocol: {}", other),
                    }
                    if let Protocol::Udp(port) = iter.next().expect("port") {
                        assert_ne!(0, port)
                    } else {
                        panic!("No UDP port in address: {}", a)
                    }
                    futures::future::ready(true)
                }
                _ => futures::future::ready(false),
            })
            .for_each(|_| futures::future::ready(()));

        async_std::task::spawn(server);
        futures::executor::block_on(client).unwrap();
    }

    #[test]
    fn multiaddr_to_udp_conversion() {
        use std::net::Ipv6Addr;
        init();
        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/12345/quic"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/255.255.255.255/udp/8080/quic"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                8080,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip6/::1/udp/12345/quic".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/udp/8080/quic"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(
                    65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
                )),
                8080,
            ))
        );
    }

    #[test]
    fn communicating_between_dialer_and_listener() {
        use super::{trace, StreamMuxer};
        init();
        let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
        let mut ready_tx = Some(ready_tx);

        async_std::task::spawn(async move {
            let addr: Multiaddr = "/ip4/127.0.0.1/udp/12345/quic"
                .parse()
                .expect("bad address?");
            let quic_config = QuicConfig::default();
            let quic_endpoint = QuicEndpoint::new(&quic_config, addr.clone()).expect("I/O error");
            let mut listener = quic_endpoint.listen_on(addr).unwrap();

            loop {
                trace!("awaiting connection");
                match listener.next().await.unwrap().unwrap() {
                    ListenerEvent::NewAddress(listen_addr) => {
                        ready_tx.take().unwrap().send(listen_addr).unwrap();
                    }
                    ListenerEvent::Upgrade { upgrade, .. } => {
                        let mut muxer = upgrade.await.expect("upgrade failed");
                        let mut socket = muxer.next().await.expect("no incoming stream");

                        let mut buf = [0u8; 3];
                        log::error!("reading data from accepted stream!");
                        socket.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, [4, 5, 6]);
                        log::error!("writing data!");
                        socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();
                    }
                    _ => unreachable!(),
                }
            }
        });

        async_std::task::block_on(async move {
            let addr = ready_rx.await.unwrap();
            let quic_config = QuicConfig::default();
            let quic_endpoint = QuicEndpoint::new(
                &quic_config,
                "/ip4/127.0.0.1/udp/12346/quic".parse().unwrap(),
            )
            .unwrap();
            // Obtain a future socket through dialing
            let connection = quic_endpoint.dial(addr.clone()).unwrap().await.unwrap();
            trace!("Received a Connection: {:?}", connection);
            let mut stream = super::QuicStream {
                id: connection.open_outbound().await.expect("failed"),
                muxer: connection.clone(),
            };
            log::warn!("have a new stream!");
            stream.write_all(&[4u8, 5, 6]).await.unwrap();
            let mut buf = [0u8; 3];
            log::error!("reading data!");
            stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [1u8, 2, 3]);
        });
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv4() {
        init();
        let quic = QuicConfig::default();

        let addr = "/ip4/127.0.0.1/udp/0/quic".parse::<Multiaddr>().unwrap();
        assert!(addr.to_string().ends_with("udp/0/quic"));

        let quic = QuicEndpoint::new(&quic, addr.clone()).expect("no error");

        let new_addr = futures::executor::block_on_stream(quic.listen_on(addr).unwrap())
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv6() {
        init();
        let config = QuicConfig::default();

        let addr: Multiaddr = "/ip6/::1/udp/0/quic".parse().unwrap();
        assert!(addr.to_string().contains("udp/0/quic"));
        let quic = QuicEndpoint::new(&config, addr.clone()).expect("no error");

        let new_addr = futures::executor::block_on_stream(quic.listen_on(addr).unwrap())
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn larger_addr_denied() {
        init();
        let config = QuicConfig::default();
        let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(QuicEndpoint::new(&config, addr).is_err())
    }
}
