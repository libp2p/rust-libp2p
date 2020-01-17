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
use crate::{EndpointMessage, QuicConfig};
use async_macros::ready;
use async_std::net::{SocketAddr, UdpSocket};
use futures::{channel::mpsc, prelude::*};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    Transport,
};
use log::{debug, error, trace, warn};
use parking_lot::{Mutex, MutexGuard};
use quinn_proto::{Connection, ConnectionHandle};
use std::{
    collections::HashMap,
    io,
    pin::Pin,
    sync::{Arc, Weak},
    task::{
        Context,
        Poll::{self, Pending, Ready},
    },
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(super) struct EndpointInner {
    inner: quinn_proto::Endpoint,
    muxers: HashMap<ConnectionHandle, Weak<Mutex<super::Muxer>>>,
    driver: Option<async_std::task::JoinHandle<Result<(), io::Error>>>,
    /// Pending packet
    outgoing_packet: Option<quinn_proto::Transmit>,
    /// Used to receive events from connections
    event_receiver: mpsc::Receiver<EndpointMessage>,
    /// Bad timer
    bad_timer: futures_timer::Delay,
}

impl EndpointInner {
    pub(super) fn handle_event(
        &mut self,
        handle: quinn_proto::ConnectionHandle,
        event: quinn_proto::EndpointEvent,
    ) -> Option<quinn_proto::ConnectionEvent> {
        if event.is_drained() {
            self.muxers.remove_entry(&handle);
        }
        self.inner.handle_event(handle, event)
    }

    fn drive_events(&mut self, cx: &mut Context) {
        while let Ready(e) = self.event_receiver.poll_next_unpin(cx).map(|e| e.unwrap()) {
            match e {
                EndpointMessage::ConnectionAccepted => {
                    debug!("accepting connection!");
                    self.inner.accept();
                }
                EndpointMessage::EndpointEvent { handle, event } => {
                    debug!("we have an event from connection {:?}", handle);
                    match self.muxers.get(&handle).and_then(|e| e.upgrade()) {
                        None => drop(self.muxers.remove(&handle)),
                        Some(connection) => match self.inner.handle_event(handle, event) {
                            None => connection.lock().process_endpoint_communication(self),
                            Some(event) => connection.lock().process_connection_events(self, event),
                        },
                    }
                }
            }
        }
    }

    fn drive_receive(
        &mut self,
        socket: &UdpSocket,
        cx: &mut Context,
    ) -> Poll<Result<(ConnectionHandle, Connection), io::Error>> {
        use quinn_proto::DatagramEvent;
        let mut buf = vec![0; 65535];
        trace!("endpoint polling for incoming packets!");
        loop {
            let (bytes, peer) = ready!(socket.poll_recv_from(cx, &mut buf[..]))?;
            trace!("got a packet of length {} from {}!", bytes, peer);
            let (handle, event) =
                match self
                    .inner
                    .handle(Instant::now(), peer, None, buf[..bytes].into())
                {
                    Some(e) => e,
                    None => continue,
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
                }
                DatagramEvent::NewConnection(connection) => {
                    debug!("new connection detected!");
                    break Ready(Ok((handle, connection)));
                }
            }
            trace!("event processed!")
        }
    }

    fn poll_transmit_pending(
        &mut self,
        socket: &UdpSocket,
        cx: &mut Context,
    ) -> Result<(), io::Error> {
        self.poll_transmit_pending_raw(socket, cx).map_err(|e| {
            error!("I/O error: {:?}", e);
            e
        })
    }

    fn poll_transmit_pending_raw(
        &mut self,
        socket: &UdpSocket,
        cx: &mut Context,
    ) -> Result<(), io::Error> {
        if let Some(tx) = self.outgoing_packet.take() {
            if self.poll_transmit(socket, cx, tx)? {
                return Ok(());
            }
        }
        while let Some(tx) = self.inner.poll_transmit() {
            if self.poll_transmit(socket, cx, tx)? {
                break;
            }
        }
        Ok(())
    }

    fn poll_transmit(
        &mut self,
        socket: &UdpSocket,
        cx: &mut Context,
        packet: quinn_proto::Transmit,
    ) -> Result<bool, io::Error> {
        loop {
            break match socket.poll_send_to(cx, &packet.contents, &packet.destination) {
                Pending => {
                    self.outgoing_packet = Some(packet);
                    Ok(true)
                }
                Ready(Ok(size)) => {
                    trace!("sent packet of length {} to {}", size, packet.destination);
                    Ok(false)
                }
                Ready(Err(e)) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                Ready(Err(e)) => Err(e),
            };
        }
    }
}

#[derive(Debug)]
struct EndpointData {
    /// The single UDP socket used for I/O
    socket: UdpSocket,
    /// A `Mutex` protecting the QUIC state machine.
    inner: Mutex<EndpointInner>,
    /// The channel on which new connections are sent.  This is bounded in practice by the accept
    /// backlog.
    new_connections: mpsc::UnboundedSender<Result<ListenerEvent<super::QuicUpgrade>, io::Error>>,
    /// The channel used to receive new connections.
    receive_connections: Mutex<
        Option<mpsc::UnboundedReceiver<Result<ListenerEvent<super::QuicUpgrade>, io::Error>>>,
    >,
    /// Connections send their events to this
    event_channel: mpsc::Sender<EndpointMessage>,
    /// The `Multiaddr`
    address: Multiaddr,
    /// The configuration
    config: QuicConfig,
}

/// A QUIC endpoint.  Each endpoint has its own configuration and listening socket.
///
/// You generally need only one of these per process.  Endpoints are `Send` and `Sync`, so you
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
pub struct Endpoint(Arc<EndpointData>);

impl Endpoint {
    fn inner(&self) -> MutexGuard<'_, EndpointInner> {
        trace!("acquiring lock!");
        let q = self.0.inner.lock();
        trace!("lock acquired!");
        q
    }

    pub(super) fn event_channel(&self) -> mpsc::Sender<EndpointMessage> {
        self.0.event_channel.clone()
    }

    pub(super) fn poll_send_to(
        &self,
        cx: &mut Context,
        transmit: &[u8],
        destination: &SocketAddr,
    ) -> Poll<Result<usize, io::Error>> {
        self.0.socket.poll_send_to(cx, transmit, destination)
    }

    /// Construct a `Endpoint` with the given `QuicConfig` and `Multiaddr`.
    pub fn new(
        config: QuicConfig,
        address: Multiaddr,
    ) -> Result<Self, TransportError<<&'static Self as Transport>::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&address) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(address));
        };
        // NOT blocking, as per man:bind(2), as we pass an IP address.
        let socket = std::net::UdpSocket::bind(&socket_addr)?.into();
        let (new_connections, receive_connections) = mpsc::unbounded();
        let (event_channel, event_receiver) = mpsc::channel(0);
        Ok(Self(Arc::new(EndpointData {
            socket,
            inner: Mutex::new(EndpointInner {
                inner: quinn_proto::Endpoint::new(
                    config.endpoint_config.clone(),
                    Some(config.server_config.clone()),
                ),
                muxers: HashMap::new(),
                driver: None,
                event_receiver,
                outgoing_packet: None,
                bad_timer: futures_timer::Delay::new(Duration::new(1, 0)),
            }),
            address,
            receive_connections: Mutex::new(Some(receive_connections)),
            new_connections,
            event_channel,
            config,
        })))
    }

    fn create_muxer(
        &self,
        connection: Connection,
        handle: ConnectionHandle,
        inner: &mut EndpointInner,
    ) -> super::QuicMuxer {
        let (driver, muxer) =
            super::ConnectionDriver::new(super::Muxer::new(self.clone(), connection, handle));
        inner.muxers.insert(handle, Arc::downgrade(&muxer));
        (*muxer).lock().driver = Some(async_std::task::spawn(driver));
        super::QuicMuxer(muxer)
    }

    fn accept_muxer(
        &self,
        connection: Connection,
        handle: ConnectionHandle,
        inner: &mut EndpointInner,
    ) {
        let muxer = self.create_muxer(connection, handle, &mut *inner);
        self.0
            .new_connections
            .unbounded_send(Ok(ListenerEvent::Upgrade {
                upgrade: super::QuicUpgrade { muxer: Some(muxer) },
                local_addr: self.0.address.clone(),
                remote_addr: self.0.address.clone(),
            }))
            .expect(
                "this is an unbounded channel, and we have an instance \
                 of the peer, so this will never fail; qed",
            );
    }
}

impl Future for Endpoint {
    type Output = Result<(), io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();
        loop {
            let mut inner = this.inner();
            inner.drive_events(cx);
            while inner.bad_timer.poll_unpin(cx).is_ready() {
                inner.bad_timer = futures_timer::Delay::new(Duration::new(1, 0));
            }
            match inner.drive_receive(&this.0.socket, cx) {
                Pending => {
                    inner.poll_transmit_pending(&this.0.socket, cx)?;
                    break Pending;
                }
                Ready(Ok((handle, connection))) => {
                    this.accept_muxer(connection, handle, &mut *inner);
                    inner.poll_transmit_pending(&this.0.socket, cx)?;
                }
                Ready(Err(e)) => {
                    log::error!("I/O error: {:?}", e);
                    return Ready(Err(e));
                }
            }
        }
    }
}

impl Transport for &Endpoint {
    type Output = super::QuicMuxer;
    type Error = io::Error;
    type Listener =
        mpsc::UnboundedReceiver<Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>>;
    type ListenerUpgrade = super::QuicUpgrade;
    type Dial = super::QuicUpgrade;

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
            inner.driver = Some(async_std::task::spawn(self.clone()));
            self.0
                .new_connections
                .unbounded_send(Ok(ListenerEvent::NewAddress(addr)))
                .expect("we have a reference to the peer, so this will not fail; qed");
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
            .connect(
                self.0.config.client_config.clone(),
                socket_addr,
                "localhost",
            )
            .map_err(|e| {
                warn!("Connection error: {:?}", e);
                TransportError::Other(io::ErrorKind::InvalidInput.into())
            });
        let (handle, conn) = s?;
        let muxer = self.create_muxer(conn, handle, &mut inner);
        Ok(super::QuicUpgrade { muxer: Some(muxer) })
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

#[test]
fn multiaddr_to_udp_conversion() {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    crate::tests::init();
    assert!(
        multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap()).is_err()
    );

    assert!(
        multiaddr_to_socketaddr(&"/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap()).is_err()
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
