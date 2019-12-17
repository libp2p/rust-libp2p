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
//! let quic_config = QuicConfig::new();
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
//! `QuicEndpoint` manages a background task that processes all socket I/O.  This includes:

use async_macros::ready;
use async_std::net::UdpSocket;
use futures::{
    channel::{mpsc, oneshot},
    prelude::*,
};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    StreamMuxer, Transport,
};
use log::debug;
use quinn_proto::{Connection, ConnectionEvent, ConnectionHandle, Dir, StreamId};
use std::{
    collections::HashMap,
    fmt, io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, Weak},
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
#[derive(Debug, Clone, Default)]
pub struct QuicConfig {
    /// The client configuration.  Quinn provides functions for making one.
    pub client_config: quinn_proto::ClientConfig,
    /// The server configuration.  Quinn provides functions for making one.
    pub server_config: Arc<quinn_proto::ServerConfig>,
    /// The endpoint configuration
    pub endpoint_config: Arc<quinn_proto::EndpointConfig>,
}

impl QuicConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> Self {
        Self::default()
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
    /// Channel to request I/O
    sender: mpsc::Sender<quinn_proto::Transmit>,
}

#[derive(Debug)]
struct EndpointInner {
    inner: quinn_proto::Endpoint,
    muxers: HashMap<ConnectionHandle, Weak<Mutex<Muxer>>>,
    driver: Option<async_std::task::JoinHandle<Result<(), io::Error>>>,
}

#[derive(Debug)]
struct Endpoint {
    /// The single UDP socket used for I/O
    socket: UdpSocket,
    /// A `Mutex` protecting the QUIC state machine.
    inner: Mutex<EndpointInner>,
    /// A channel used to receive UDP packets from the `Muxer`s.
    receiver: mpsc::Receiver<quinn_proto::Transmit>,
    /// The sending side of said channel.  A clone of this is included in every `Muxer` created by
    /// this `Endpoint`.
    sender: mpsc::Sender<quinn_proto::Transmit>,
    /// The channel on which new connections are sent.  This is bounded in practice by the accept
    /// backlog.
    new_connections: mpsc::UnboundedSender<Result<ListenerEvent<QuicUpgrade>, io::Error>>,
    /// The channel used to receive new connections.
    receive_connections:
        Mutex<Option<mpsc::UnboundedReceiver<Result<ListenerEvent<QuicUpgrade>, io::Error>>>>,
    /// The `Multiaddr`
    address: Multiaddr,
}

impl Muxer {
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

    /// Process application events
    fn process_app_events(&mut self, endpoint: &mut EndpointInner, event: ConnectionEvent) {
        use quinn_proto::Event;
        self.connection.handle_event(event);
        self.send_to_endpoint(endpoint);
        while let Some(event) = self.connection.poll() {
            match event {
                Event::DatagramSendUnblocked => {
                    panic!("we never try to send datagrams, so this will never happen; qed")
                }
                Event::StreamOpened { dir: Dir::Uni } => { /* do nothing */ }
                Event::StreamAvailable { dir: Dir::Uni } | Event::DatagramReceived => continue,
                Event::StreamReadable { stream } => {
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.readers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamWritable { stream } => {
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.writers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                Event::StreamAvailable { dir: Dir::Bi } => {
                    if self.connectors.is_empty() {
                        // no task to wake up
                        return;
                    }
                    debug_assert!(
                        self.pending_stream.is_none(),
                        "we cannot have both pending tasks and a pending stream; qed"
                    );
                    let stream = self.connection.open(Dir::Bi)
						.expect("we just were told that there is a stream available; there is a mutex that prevents other threads from calling open() in the meantime; qed");
                    while let Some(oneshot) = self.connectors.pop_front() {
                        match oneshot.send(stream) {
                            Ok(()) => return,
                            Err(_) => (),
                        }
                    }
                    self.pending_stream = Some(stream)
                }
                Event::ConnectionLost { .. } => break, // nothing more
                Event::StreamFinished { stream, .. } => {
                    // Wake up the task waiting on us (if any)
                    if let Some((_, waker)) = self.writers.remove_entry(&stream) {
                        waker.wake()
                    }
                }
                // These are separate events, but are handled the same way.
                Event::StreamOpened { dir: Dir::Bi } | Event::Connected => {
                    if let Some(w) = self.accept_waker.take() {
                        w.wake()
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuicMuxer(Arc<Mutex<Muxer>>);

impl QuicMuxer {
    fn inner<'a>(&'a self) -> MutexGuard<'a, Muxer> {
        self.0
            .lock()
            .expect("we already panicked, so are in an inconsistent state; qed")
    }
}

#[derive(Debug)]
enum OutboundInner {
    Complete(StreamId),
    Pending(oneshot::Receiver<StreamId>),
}

pub struct Outbound(OutboundInner);

impl Future for Outbound {
    type Output = Result<StreamId, std::io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.get_mut().0 {
            OutboundInner::Complete(s) => Poll::Ready(Ok(s)),
            OutboundInner::Pending(ref mut receiver) => Pin::new(receiver)
                .poll(cx)
                .map_err(|oneshot::Canceled| std::io::ErrorKind::ConnectionAborted.into()),
        }
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = Outbound;
    type Substream = StreamId;
    type Error = std::io::Error;
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let mut inner = self.inner();
        if let Some(id) = inner.pending_stream.take() {
            // mandatory ― otherwise we will fail an assertion above
            Outbound(OutboundInner::Complete(id))
        } else if let Some(id) = inner.connection.open(Dir::Bi) {
            // optimization: if we can complete synchronously, do so.
            Outbound(OutboundInner::Complete(id))
        } else {
            let (sender, receiver) = oneshot::channel();
            inner.connectors.push_front(sender);
            Outbound(OutboundInner::Pending(receiver))
        }
    }
    fn destroy_outbound(&self, _: Outbound) {}
    fn destroy_substream(&self, _substream: Self::Substream) {}
    fn is_remote_acknowledged(&self) -> bool {
        true
    }

    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut inner = self.inner();
        match inner.connection.accept(quinn_proto::Dir::Bi) {
            None => {
                inner.accept_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            Some(stream) => Poll::Ready(Ok(stream)),
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
        ready!(inner.sender.poll_ready(cx))
            .expect("we have a strong reference to the other end, so it won’t be dropped; qed");
        match inner.connection.write(*substream, buf) {
            Ok(bytes) => {
                if let Some(transmit) = inner.connection.poll_transmit(Instant::now()) {
                    // UDP is unreliable, so the occasional dropped packet is fine.
                    drop(inner.sender.start_send(transmit))
                }
                Poll::Ready(Ok(bytes))
            }

            Err(WriteError::Blocked) => {
                inner.writers.insert(*substream, cx.waker().clone());
                Poll::Pending
            }
            Err(WriteError::UnknownStream) => {
                panic!("libp2p never uses a closed stream, so this cannot happen; qed")
            }
            Err(WriteError::Stopped { error_code: _ }) => {
                Poll::Ready(Err(std::io::ErrorKind::ConnectionAborted.into()))
            }
        }
    }

    fn poll_outbound(
        &self,
        cx: &mut Context,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        Pin::new(substream).poll(cx)
    }

    fn read_substream(
        &self,
        cx: &mut Context,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        use quinn_proto::ReadError;
        let mut inner = self.inner();
        ready!(inner.sender.poll_ready(cx))
            .expect("we have a strong reference to the other end, so it won’t be dropped; qed");
        match inner.connection.read(*substream, buf) {
            Ok(Some(bytes)) => {
                if let Some(transmit) = inner.connection.poll_transmit(Instant::now()) {
                    // UDP is unreliable, so the occasional dropped packet is fine.
                    drop(inner.sender.start_send(transmit))
                }
                Poll::Ready(Ok(bytes))
            }
            Ok(None) => Poll::Ready(Ok(0)),
            Err(ReadError::Blocked) => {
                inner.readers.insert(*substream, cx.waker().clone());
                Poll::Pending
            }
            Err(ReadError::UnknownStream) => {
                panic!("libp2p never uses a closed stream, so this cannot happen; qed")
            }
            Err(ReadError::Reset { error_code: _ }) => {
                Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()))
            }
        }
    }

    fn shutdown_substream(
        &self,
        _cx: &mut Context,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(
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
        Poll::Ready(Ok(()))
    }

    fn close(&self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(self.inner().connection.close(
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
        Poll::Ready(Ok(()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
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
        self.0.inner.lock().unwrap()
    }

    /// Retrieves the `Multiaddr` of this `QuicEndpoint`.
    pub fn addr(&self) -> &Multiaddr {
        &self.0.address
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
        let (sender, receiver) = mpsc::channel(0);
        let (new_connections, receive_connections) = mpsc::unbounded();
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
                }),
                sender,
                receiver,
                new_connections,
                receive_connections: Mutex::new(Some(receive_connections)),
                address,
            }),
            config.clone(),
        ))
    }

    fn create_muxer(
        &self,
        connection: Connection,
        handle: ConnectionHandle,
        inner: &mut EndpointInner,
    ) -> Arc<Mutex<Muxer>> {
        let endpoint = &self.0;
        let muxer = Arc::new(Mutex::new(Muxer {
            pending_stream: None,
            connection,
            handle,
            writers: HashMap::new(),
            readers: HashMap::new(),
            accept_waker: None,
            connectors: Default::default(),
            sender: endpoint.sender.clone(),
            endpoint: endpoint.clone(),
        }));
        inner.muxers.insert(handle, Arc::downgrade(&muxer));
        muxer
    }

    /// Process incoming UDP packets until an error occurs on the socket, or we are dropped.
    async fn process_incoming_packets(self, address: Multiaddr) -> Result<(), io::Error> {
        self.0
            .new_connections
            .unbounded_send(Ok(ListenerEvent::NewAddress(address)))
            .expect("we have a reference to the peer, so this will not fail; qed");
        loop {
            use quinn_proto::DatagramEvent;
            let mut buf = [0; 1800];
            let (bytes, peer) = self.0.socket.recv_from(&mut buf[..]).await?;
            // This takes a mutex, so it must be *after* the `await` call.
            let mut inner = self.inner();
            let (handle, event) =
                match inner
                    .inner
                    .handle(Instant::now(), peer, None, buf[..bytes].into())
                {
                    Some(e) => e,
                    None => continue,
                };
            match event {
                DatagramEvent::ConnectionEvent(connection_event) => {
                    let connection = match inner.muxers.get(&handle) {
                        None => panic!("received a ConnectionEvent for an unknown Connection"),
                        Some(e) => match e.upgrade() {
                            Some(e) => e,
                            None => continue, // FIXME should this be a panic?
                        },
                    };
                    let mut connection = connection
                        .lock()
                        .expect("we assume we have not already panicked; qed");
                    connection.process_app_events(&mut inner, connection_event);
                }
                DatagramEvent::NewConnection(connection) => {
                    let muxer = self.create_muxer(connection, handle, &mut *inner);
                    let endpoint = &self.0;
                    endpoint
						.new_connections
						.unbounded_send(Ok(ListenerEvent::Upgrade {
							upgrade: QuicUpgrade {
								muxer: Some(QuicMuxer(muxer)),
							},
							local_addr: endpoint.address.clone(),
							remote_addr: endpoint.address.clone(),
						}))
					.expect(
						"this is an unbounded channel, and we have an instance of the peer, so \
					 this will never fail; qed",
					);
                }
            }
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
        {
            let mut connection = muxer.as_mut().expect("polled after yielding Ready").inner();
            if connection.connection.is_handshaking() {
                connection.accept_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }
        Poll::Ready(Ok(muxer.take().expect("impossible")))
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
            .unwrap()
            .take()
            .ok_or_else(|| TransportError::Other(io::ErrorKind::AlreadyExists.into()));
        let mut inner = self.inner();
        if inner.driver.is_none() {
            inner.driver = Some(async_std::task::spawn(
                self.clone().process_incoming_packets(addr),
            ))
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
            inner.driver = Some(async_std::task::spawn(
                self.clone()
                    .process_incoming_packets(self.0.address.clone()),
            ))
        }

        let s: Result<(_, Connection), _> = inner
            .inner
            .connect(self.1.client_config.clone(), socket_addr, "localhost")
            .map_err(|e| {
                eprintln!("Connection error: {:?}", e);
                TransportError::Other(io::ErrorKind::InvalidInput.into())
            });
        let (handle, conn) = s?;
        Ok(QuicUpgrade {
            muxer: Some(QuicMuxer(self.create_muxer(conn, handle, &mut inner))),
        })
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

    #[test]
    #[ignore] // this needs cmsg support from async-std
    fn wildcard_expansion() {
        let addr: Multiaddr = "/ip4/0.0.0.0/udp/1234/quic".parse().unwrap();
        let listener = QuicEndpoint::new(&QuicConfig::new(), addr.clone())
            .expect("endpoint")
            .listen_on(addr)
            .expect("listener");
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/1236/quic".parse().unwrap();
        let client = QuicEndpoint::new(&QuicConfig::new(), addr.clone())
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
                        Protocol::Ip4(ip) => assert!(!ip.is_unspecified()),
                        Protocol::Ip6(ip) => assert!(!ip.is_unspecified()),
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

        futures::executor::block_on(futures::future::join(server, client))
            .1
            .unwrap();
    }

    #[test]
    fn multiaddr_to_udp_conversion() {
        use std::net::Ipv6Addr;

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
        let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
        let mut ready_tx = Some(ready_tx);

        async_std::task::spawn(async move {
            let addr: Multiaddr = "/ip4/127.0.0.1/udp/12345/quic"
                .parse()
                .expect("bad address?");
            let quic_config = QuicConfig::new();
            let quic_endpoint = QuicEndpoint::new(&quic_config, addr.clone()).expect("I/O error");
            let mut listener = quic_endpoint.listen_on(addr).unwrap();

            loop {
                match listener.next().await.unwrap().unwrap() {
                    ListenerEvent::NewAddress(listen_addr) => {
                        ready_tx.take().unwrap().send(listen_addr).unwrap();
                    }
                    ListenerEvent::Upgrade { upgrade, .. } => {
                        let mut upgrade = upgrade.await.unwrap().next().await.unwrap();
                        let mut buf = [0u8; 3];
                        upgrade.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, [1, 2, 3]);
                        upgrade.write_all(&[4, 5, 6]).await.unwrap();
                    }
                    _ => unreachable!(),
                }
            }
        });

        async_std::task::block_on(async move {
            let addr = ready_rx.await.unwrap();
            let quic_config = QuicConfig::new();
            let quic_endpoint = QuicEndpoint::new(
                &quic_config,
                "/ip4/127.0.0.1/udp/12346/quic".parse().unwrap(),
            )
            .unwrap();
            // Obtain a future socket through dialing
            let mut connection = quic_endpoint.dial(addr.clone()).unwrap().await.unwrap();
            eprintln!("Received a Connection: {:?}", connection);
            let mut socket = connection.next().await.unwrap();
            socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

            let mut buf = [0u8; 3];
            socket.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [4, 5, 6]);
        });
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv4() {
        let quic = QuicConfig::new();

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
        let config = QuicConfig::new();

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
        let config = QuicConfig::new();
        let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(QuicEndpoint::new(&config, addr).is_err())
    }
}
