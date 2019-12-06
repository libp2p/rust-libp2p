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
use futures::{channel::mpsc, future, prelude::*};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    StreamMuxer, Transport,
};
use log::debug;
use quinn_proto::{Connection, ConnectionEvent, ConnectionHandle, Dir, StreamId};
use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, Weak},
    task::{Context, Poll},
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

pub struct QuicSubstream {
    stream: StreamId,
    transmit: Option<quinn_proto::Transmit>,
}

impl QuicConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug)]
pub struct Muxer {
    endpoint: Arc<Endpoint>,
    connection: Connection,
    /// Connection handle
    handle: ConnectionHandle,
    /// Tasks blocked on writing
    writers: HashMap<StreamId, std::task::Waker>,
    /// Tasks blocked on reading
    readers: HashMap<StreamId, std::task::Waker>,
    /// Tasks waiting for new connections
    acceptors: Vec<std::task::Waker>,
    /// Tasks waiting to make a connection
    connectors: Vec<std::task::Waker>,
    /// Channel to request I/O
    sender: mpsc::Sender<quinn_proto::Transmit>,
}

#[derive(Debug)]
struct EndpointInner {
    inner: quinn_proto::Endpoint,
    muxers: HashMap<ConnectionHandle, Weak<Mutex<Muxer>>>,
}

/// A QUIC endpoint.  You generally need only one of these per process.  However, performance may
/// be better if you have one per CPU core.
#[derive(Debug)]
pub struct Endpoint {
    /// The single UDP socket used for I/O
    socket: UdpSocket,
    /// A `Mutex` protecting the QUIC state machine.
    inner: Mutex<EndpointInner>,
    /// A channel used to receive messages from the `Muxer`s.
    receiver: mpsc::Receiver<quinn_proto::Transmit>,
    /// The sending side of said channel.  A clone of this is included in every `Muxer` created by
    /// this `Endpoint`.
    sender: mpsc::Sender<quinn_proto::Transmit>,
    /// The channel on which new connections are sent.  This is bounded in practice by the accept
    /// backlog.
    new_connections: mpsc::UnboundedSender<QuicMuxer>,
    /// The channel used to receive new connections.
    receive_connections: mpsc::UnboundedReceiver<QuicMuxer>,
}

impl Muxer {
    /// Send all outgoing packets
    async fn transmit(&mut self) -> Result<(), async_std::io::Error> {
        while let Some(quinn_proto::Transmit {
            destination,
            ecn,
            contents,
        }) = self.connection.poll_transmit(Instant::now())
        {
            drop(ecn);
            self.endpoint
                .socket
                .send_to(&*contents, destination)
                .await?;
        }
        Ok(())
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

    /// Process application events
    fn process_app_events(&mut self, endpoint: &mut EndpointInner) {
        use quinn_proto::Event;
        self.send_to_endpoint(endpoint);
        while let Some(event) = self.connection.poll() {
            match event {
                Event::Connected => {
                    panic!("is not emitted more than once; we already received it; qed")
                }
                Event::DatagramSendUnblocked => {
                    panic!("we never try to send datagrams, so this will never happen; qed")
                }
                Event::StreamOpened { dir: Dir::Uni } => {
                    let id = self
                        .connection
                        .accept(Dir::Uni)
                        .expect("we just received an incoming connection; qed");
                    self.connection
                        .stop_sending(id, Default::default())
                        .expect("we just accepted this stream, so we know it; qed")
                }
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
                    let _: Option<()> = self.acceptors.pop().map(|w| w.wake());
                }
                Event::ConnectionLost { .. } => break, // nothing more
                Event::StreamFinished { .. } => unimplemented!(),
                Event::StreamOpened { dir: Dir::Bi } => {
                    let _: Option<()> = self.connectors.pop().map(|w| w.wake());
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct QuicMuxer(Arc<Mutex<Muxer>>);

impl QuicMuxer {
    fn inner<'a>(&'a self) -> MutexGuard<'a, Muxer> {
        self.0
            .lock()
            .expect("we already panicked, so are in an inconsistent state; qed")
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = ();
    type Substream = StreamId;
    type Error = std::io::Error;
    fn open_outbound(&self) {}
    fn destroy_outbound(&self, (): ()) {}
    fn destroy_substream(&self, substream: Self::Substream) {}
    fn is_remote_acknowledged(&self) -> bool {
        true
    }

    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, Self::Error>> {
        let mut inner = self.inner();
        match inner.connection.accept(quinn_proto::Dir::Bi) {
            None => {
                inner.acceptors.push(cx.waker().clone());
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
        let mut inner = self.inner();
        match inner.connection.open(Dir::Bi) {
            None => {
                inner.connectors.push(cx.waker().clone());
                Poll::Pending
            }
            Some(id) => Poll::Ready(Ok(id)),
        }
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
        cx: &mut Context,
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

    fn close(&self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(self.inner().connection.close(
            Instant::now(),
            Default::default(),
            Default::default(),
        )))
    }
}

impl Stream for Endpoint {
    type Item = Result<QuicMuxer, io::Error>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut inner = self.inner.lock().unwrap();
        // inner.pending_connections -= 1;
        inner.inner.accept();
        unimplemented!()
    }
}

impl Endpoint {
    fn inner(&self) -> MutexGuard<'_, EndpointInner> {
        self.inner
            .lock()
            .expect("we don’t panic here unless something has already gone horribly wrong")
    }
}

#[derive(Debug, Clone)]
pub struct QuicEndpoint(Arc<Endpoint>, Multiaddr);

impl QuicEndpoint {
    fn inner(&self) -> MutexGuard<'_, EndpointInner> {
        self.0.inner.lock().unwrap()
    }

    pub fn new(
        config: &QuicConfig,
        addr: Multiaddr,
    ) -> Result<Self, TransportError<<Self as Transport>::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        // NOT blocking, as per man:bind(2)
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
                }),
                sender,
                receiver,
                new_connections,
                receive_connections,
            }),
            addr,
        ))
    }

    /// Process incoming UDP packets until an error occurs on the socket, or we are dropped.
    async fn process_packet(&self) -> Result<(), io::Error> {
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
            let connection = match event {
                DatagramEvent::ConnectionEvent(connection_event) => {
                    let connection = match inner.muxers.get(&handle) {
                        None => panic!("received a ConnectionEvent for an unknown Connection"),
                        Some(e) => match e.upgrade() {
                            Some(e) => e,
                            None => continue, // FIXME should this be a panic?
                        },
                    };
                    (*connection).lock().unwrap().process_app_events(&mut inner);
                    continue;
                }
                DatagramEvent::NewConnection(connection) => {
                    drop(inner);
                    connection
                }
            };
            let endpoint = &self.0;
            let muxer = Arc::new(Mutex::new(Muxer {
                connection,
                handle,
                writers: HashMap::new(),
                readers: HashMap::new(),
                acceptors: vec![],
                connectors: vec![],
                sender: endpoint.sender.clone(),
                endpoint: endpoint.clone(),
            }));
            self.inner().muxers.insert(handle, Arc::downgrade(&muxer));
            self.inner().muxers.insert(handle, Arc::downgrade(&muxer));
            endpoint.new_connections
                .unbounded_send(QuicMuxer(muxer))
                .expect(
                    "this is an unbounded channel, and we have an instance of the peer, so \
                     this will never fail; qed",
                );
        }
    }
}

pub struct QuicConnecting {
    endpoint: Arc<Endpoint>,
}

impl Future for QuicConnecting {
    type Output = Result<QuicMuxer, io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        unimplemented!()
    }
}

impl Transport for QuicEndpoint {
    type Output = QuicMuxer;
    type Error = io::Error;
    type Listener = mpsc::Receiver<Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>>;
    type ListenerUpgrade = future::Ready<Result<QuicMuxer, Self::Error>>;
    type Dial = QuicConnecting;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        if addr != self.1 {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        unimplemented!()
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

        unimplemented!()
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

#[cfg(any())]
#[cfg(test)]
mod tests {
    use super::{multiaddr_to_socketaddr, QuicConfig};
    use futures::prelude::*;
    use libp2p_core::{
        multiaddr::{Multiaddr, Protocol},
        transport::ListenerEvent,
        Transport,
    };
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn wildcard_expansion() {
        let mut listener = QuicConfig::new()
            .listen_on("/ip4/0.0.0.0/udp/0/quic".parse().unwrap())
            .expect("listener");

        // Get the first address.
        let addr = futures::executor::block_on_stream(listener.by_ref())
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

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

        let client = QuicConfig::new().dial(addr).expect("dialer");
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

    #[cfg(any())]
    #[test]
    fn communicating_between_dialer_and_listener() {
        let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
        let mut ready_tx = Some(ready_tx);

        async_std::task::spawn(async move {
            let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
            let tcp = QuicConfig::new();
            let mut listener = tcp.listen_on(addr).unwrap();

            loop {
                match listener.next().await.unwrap().unwrap() {
                    ListenerEvent::NewAddress(listen_addr) => {
                        ready_tx.take().unwrap().send(listen_addr).unwrap();
                    }
                    ListenerEvent::Upgrade { upgrade, .. } => {
                        let mut upgrade = upgrade.await.unwrap();
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
            let tcp = QuicConfig::new();

            // Obtain a future socket through dialing
            let mut socket = tcp.dial(addr.clone()).unwrap().await.unwrap();
            socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

            let mut buf = [0u8; 3];
            socket.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [4, 5, 6]);
        });
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv4() {
        let tcp = QuicConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = futures::executor::block_on_stream(tcp.listen_on(addr).unwrap())
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv6() {
        let tcp = QuicConfig::new();

        let addr: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = futures::executor::block_on_stream(tcp.listen_on(addr).unwrap())
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn larger_addr_denied() {
        let tcp = QuicConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(tcp.listen_on(addr).is_err());
    }
}
