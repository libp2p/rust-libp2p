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
//! extern crate libp2p_tcp;
//! use libp2p_tcp::QuicConfig;
//!
//! # fn main() {
//! let tcp = QuicConfig::new();
//! # }
//! ```
//!
//! The `QuicConfig` structs implements the `Transport` trait of the `swarm` library. See the
//! documentation of `swarm` and of libp2p in general to learn how to use the `Transport` trait.
//!
//! Note that QUIC provides transport, security, and multiplexing in a single protocol.  Therefore,
//! QUIC connections do not need to be upgraded.  You will get a compile-time error if you try.
//! Instead, you must pass all needed configuration into the constructor.

use async_std::net::UdpSocket;
use futures::{channel::mpsc, future, prelude::*};
use ipnet::IpNet;
use libp2p_core::{
    multiaddr::{ip_to_multiaddr, Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    StreamMuxer, Transport,
};
use log::debug;
use quinn_proto::{Connection, ConnectionEvent, ConnectionHandle, Dir, StreamId};
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
    task::{Context, Poll},
    time::Instant,
};

/// Represents the configuration for a QUIC/UDP/IP transport capability for libp2p.
///
/// The QUIC endpoints created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone)]
pub struct QuicConfig {
    /// The client configuration.  Quinn provides functions for making one.
    pub client_configuration: quinn_proto::ClientConfig,
    /// The server configuration.  Quinn provides functions for making one.
    pub server_configuration: quinn_proto::ServerConfig,
}

impl QuicConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct Muxer {
    endpoint: Arc<Endpoint>,
    connection: Connection,
    handle: ConnectionHandle,
    channel: mpsc::UnboundedReceiver<ConnectionEvent>,
    /// Tasks blocked on writing
    writers: HashMap<StreamId, Vec<std::task::Waker>>,
    /// Tasks blocked on reading
    readers: HashMap<StreamId, Vec<std::task::Waker>>,
    /// Tasks waiting for new connections
    acceptors: Vec<std::task::Waker>,
    /// Tasks waiting to make a connection
    connectors: Vec<std::task::Waker>,
}

#[derive(Debug)]
struct Endpoint {
    inner: Mutex<quinn_proto::Endpoint>,
    socket: UdpSocket,
    hashtable: Mutex<HashMap<ConnectionHandle, mpsc::Sender<ConnectionEvent>>>,
    channel: mpsc::Sender<QuicMuxer>,
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
    fn send_to_endpoint(&mut self) {
        let mut endpoint = self.endpoint.inner.lock().unwrap();
        while let Some(endpoint_event) = self.connection.poll_endpoint_events() {
            if let Some(connection_event) = endpoint.handle_event(self.handle, endpoint_event) {
                self.connection.handle_event(connection_event)
            }
        }
    }

    /// Process application events
    fn process_app_events(&mut self) {
        use quinn_proto::Event;
        self.send_to_endpoint();
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
                Event::StreamReadable { stream } => drop(
                    self.readers
                        .get_mut(&stream)
                        .and_then(|s| s.pop())
                        .map(|w| w.wake()),
                ),
                Event::StreamWritable { stream } => drop(
                    self.writers
                        .get_mut(&stream)
                        .and_then(|s| s.pop())
                        .map(|w| w.wake()),
                ),
                Event::StreamAvailable { dir: Dir::Bi } => {
                    drop(self.acceptors.pop().map(|w| w.wake()))
                }
                Event::ConnectionLost { .. } => break, // nothing more
                Event::StreamFinished { .. } => unimplemented!(),
                Event::StreamOpened { dir: Dir::Bi } => {
                    drop(self.connectors.pop().map(|w| w.wake()))
                }
            }
        }
    }

    async fn process(&mut self) {
        while let Some(s) = self.channel.next().await {
            self.connection.handle_event(s)
        }
    }
}

#[derive(Debug)]
pub enum FutureMuxer {
    Connecting(Option<std::task::Waker>, Muxer),
    Connected,
}

impl Future for FutureMuxer {
    type Output = Result<QuicMuxer, io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let inner: &mut _ = Pin::into_inner(self);
        match inner {
            FutureMuxer::Connected => panic!("polled after yielding Ready"),
            FutureMuxer::Connecting(ref mut waker, _) => unimplemented!(),
        }
    }
    // 		self.0.
    // 		let FutureMuxer(ref mut muxer) = self.into_inner();
    // 		let muxer = muxer.as_mut().expect("polled after yielding Ready");
    // 		match muxer.connection.poll() {
    // 			Some(Event::Connected) => true,
    // 			Some(_) => panic!("`Event::Connected` is the first event emitted; we have not seen it yet, so we will not see any other events; qed"),
    // 			None =>
    //
    //
    // 		if self.has_connected {
    // 			Poll::Ready(self.clone());
    // 		match self.as_mut().connection_waker {
    // 			None => Poll::Ready(
}

#[derive(Debug)]
pub struct QuicMuxer(Mutex<Muxer>);

impl QuicMuxer {
    fn inner<'a>(&'a self) -> MutexGuard<'a, Muxer> {
        self.0
            .lock()
            .expect("we already panicked, so are in an inconsistent state; qed")
    }
}

impl StreamMuxer for QuicMuxer {
    type OutboundSubstream = ();
    type Substream = quinn_proto::StreamId;
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
            Some(stream) => {
                inner.send_to_endpoint();
                Poll::Ready(Ok(stream))
            }
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
        match inner.connection.write(*substream, buf) {
            Ok(bytes) => {
                inner.send_to_endpoint();
                Poll::Ready(Ok(bytes))
            }
            Err(WriteError::Blocked) => {
                inner
                    .writers
                    .entry(*substream)
                    .or_insert(vec![])
                    .push(cx.waker().clone());
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
            Some(id) => {
                inner.send_to_endpoint();
                Poll::Ready(Ok(id))
            }
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
        match inner.connection.read(*substream, buf) {
            Ok(Some(bytes)) => {
                inner.send_to_endpoint();
                Poll::Ready(Ok(bytes))
            }
            Ok(None) => unimplemented!(),
            Err(ReadError::Blocked) => {
                inner
                    .readers
                    .entry(*substream)
                    .or_insert(vec![])
                    .push(cx.waker().clone());
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
        inner.accept();
        unimplemented!()
    }
}

#[derive(Debug)]
struct QuicEndpoint(Arc<Endpoint>);

impl QuicEndpoint {
    fn inner(&self) -> MutexGuard<'_, quinn_proto::Endpoint> {
        self.0.inner.lock().unwrap()
    }
}

impl Transport for QuicConfig {
    type Output = QuicMuxer;
    type Error = io::Error;
    type Listener = mpsc::Receiver<Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>>;
    type ListenerUpgrade = future::Ready<Result<QuicMuxer, Self::Error>>;
    type Dial = FutureMuxer;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        // NOT blocking, as per man:bind(2)
        let socket = std::net::UdpSocket::bind(&socket_addr)?.into();
        Endpoint {
            socket,
            inner: Mutex::new(
                quinn_proto::Endpoint::new(Default::default(), None)
                    .expect("the default config is valid; qed"),
            ),
            hashtable: Mutex::new(HashMap::new()),
            channel: unimplemented!(),
        };
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
