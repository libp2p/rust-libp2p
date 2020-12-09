// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for TCP/IP.
//!
//! # Usage
//!
//! This crate provides `TcpConfig` which implements the `Transport` trait
//! for use as a transport with `libp2p-core` or `libp2p-swarm`.

use async_io::{Async, Timer};
use futures::{
    future::{self, BoxFuture, Ready},
    prelude::*,
    ready,
};
use if_watch::{IfWatcher, IfEvent};
use libp2p_core::{
    address_translation,
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, Transport, TransportError},
};
use socket2::{Domain, Socket, Type};
use std::{
    collections::HashSet,
    io,
    net::{SocketAddr, IpAddr, TcpListener, TcpStream},
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
    time::Duration,
};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
/// The TCP sockets created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Clone, Debug)]
pub struct TcpConfig {
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
    /// Size of the listen backlog for listen sockets.
    backlog: u32,
    /// The configuration of port reuse when dialing.
    port_reuse: PortReuse,
}

type Port = u16;

/// The configuration for port reuse of listening sockets.
#[derive(Debug, Clone)]
enum PortReuse {
    /// Port reuse is disabled, i.e. ephemeral local ports are
    /// used for outgoing TCP connections.
    Disabled,
    /// Port reuse when dialing is enabled, i.e. the local
    /// address and port that a new socket for an outgoing
    /// connection is bound to are chosen from an existing
    /// listening socket, if available.
    Enabled {
        /// The addresses and ports of the listening sockets
        /// registered as eligible for port reuse when dialing.
        listen_addrs: Arc<RwLock<HashSet<(IpAddr, Port)>>>
    },
}

impl PortReuse {
    /// Registers a socket address for port reuse.
    ///
    /// Has no effect if port reuse is disabled.
    fn register(&mut self, ip: IpAddr, port: Port) {
        if let PortReuse::Enabled { listen_addrs } = self {
            log::trace!("Registering for port reuse: {}:{}", ip, port);
            listen_addrs
                .write()
                .expect("`register()` and `unregister()` never panic while holding the lock")
                .insert((ip, port));
        }
    }

    /// Unregisters a socket address for port reuse.
    ///
    /// Has no effect if port reuse is disabled.
    fn unregister(&mut self, ip: IpAddr, port: Port) {
        if let PortReuse::Enabled { listen_addrs } = self {
            log::trace!("Unregistering for port reuse: {}:{}", ip, port);
            listen_addrs
                .write()
                .expect("`register()` and `unregister()` never panic while holding the lock")
                .remove(&(ip, port));
        }
    }

    /// Selects a listening socket address suitable for use
    /// as the local socket address when dialing.
    ///
    /// If multiple listening sockets are registered for port
    /// reuse, one is chosen whose IP protocol version and
    /// loopback status is the same as that of `remote_ip`.
    ///
    /// Returns `None` if port reuse is disabled or no suitable
    /// listening socket address is found.
    fn local_dial_addr(&self, remote_ip: &IpAddr) -> Option<SocketAddr> {
        if let PortReuse::Enabled { listen_addrs } = self {
            for (ip, port) in listen_addrs
                .read()
                .expect("`register()` and `unregister()` never panic while holding the lock")
                .iter()
            {
                if ip.is_ipv4() == remote_ip.is_ipv4()
                    && ip.is_loopback() == remote_ip.is_loopback()
                {
                     return Some(SocketAddr::new(*ip, *port))
                }
            }
        }

        None
    }
}

impl TcpConfig {
    /// Creates a new configuration for a TCP/IP transport:
    ///
    ///   * Nagle's algorithm, i.e. `TCP_NODELAY`, is _enabled_.
    ///     See [`TcpConfig::nodelay`].
    ///   * Reuse of listening ports is _disabled_.
    ///     See [`TcpConfig::port_reuse`].
    ///   * No custom `IP_TTL` is set. The default of the OS TCP stack applies.
    ///     See [`TcpConfig::ttl`].
    ///   * The size of the listen backlog for new listening sockets is `1024`.
    ///     See [`TcpConfig::listen_backlog`].
    pub fn new() -> Self {
        Self {
            ttl: None,
            nodelay: None,
            backlog: 1024,
            port_reuse: PortReuse::Disabled,
        }
    }

    /// Configures the `IP_TTL` option for new sockets.
    pub fn ttl(mut self, value: u32) -> Self {
        self.ttl = Some(value);
        self
    }

    /// Configures the `TCP_NODELAY` option for new sockets.
    pub fn nodelay(mut self, value: bool) -> Self {
        self.nodelay = Some(value);
        self
    }

    /// Configures the listen backlog for new listen sockets.
    pub fn listen_backlog(mut self, backlog: u32) -> Self {
        self.backlog = backlog;
        self
    }

    /// Configures port reuse for local sockets, which implies
    /// reuse of listening ports for outgoing connections to
    /// enhance NAT traversal capabilities.
    ///
    /// Please refer to e.g. [RFC 4787](https://tools.ietf.org/html/rfc4787)
    /// section 4 and 5 for some of the NAT terminology used here.
    ///
    /// There are two main use-cases for port reuse among local
    /// sockets:
    ///
    ///   1. Creating multiple listening sockets for the same address
    ///      and port to allow accepting connections on multiple threads
    ///      without having to synchronise access to a single listen socket.
    ///
    ///   2. Creating outgoing connections whose local socket is bound to
    ///      the same address and port as a listening socket. In the rare
    ///      case of simple NATs with both endpoint-independent mapping and
    ///      endpoint-independent filtering, this can on its own already
    ///      permit NAT traversal by other nodes sharing the observed
    ///      external address of the local node. For the common case of
    ///      NATs with address-dependent or address and port-dependent
    ///      filtering, port reuse for outgoing connections can facilitate
    ///      further TCP hole punching techniques for NATs that perform
    ///      endpoint-independent mapping. Port reuse cannot facilitate
    ///      NAT traversal in the presence of "symmetric" NATs that employ
    ///      both address/port-dependent mapping and filtering, unless
    ///      there is some means of port prediction.
    ///
    /// Both use-cases are enabled when port reuse is enabled, with port reuse
    /// for outgoing connections (`2.` above) always being implied.
    ///
    /// > **Note**: Due to the identification of a TCP socket by a 4-tuple
    /// > of source IP address, source port, destination IP address and
    /// > destination port, with port reuse enabled there can be only
    /// > a single outgoing connection to a particular address and port
    /// > of a peer per local listening socket address.
    ///
    /// If enabled, the returned `TcpConfig` and all of its `Clone`s
    /// keep track of the listen socket addresses as they are reported
    /// by polling [`TcpListenStream`]s obtained from [`TcpConfig::listen_on()`].
    ///
    /// In contrast, two `TcpConfig`s constructed separately via [`TcpConfig::new()`]
    /// maintain these addresses independently. It is thus possible to listen on
    /// multiple addresses, enabling port reuse for each, knowing exactly which
    /// listen address is reused when dialing with a specific `TcpConfig`, as in
    /// the following example:
    ///
    /// ```no_run
    /// # use libp2p_core::transport::ListenerEvent;
    /// # use libp2p_tcp::TcpConfig;
    /// # use libp2p_core::{Multiaddr, Transport};
    /// # use futures::stream::StreamExt;
    /// # #[async_std::main]
    /// # async fn main() -> std::io::Result<()> {
    ///
    /// let listen_addr1: Multiaddr = "/ip4/127.0.0.1/tcp/9001".parse().unwrap();
    /// let listen_addr2: Multiaddr = "/ip4/127.0.0.1/tcp/9002".parse().unwrap();
    ///
    /// let tcp1 = TcpConfig::new().port_reuse(true);
    /// let mut listener1 = tcp1.clone().listen_on(listen_addr1.clone()).expect("listener");
    /// match listener1.next().await.expect("event")? {
    ///     ListenerEvent::NewAddress(listen_addr) => {
    ///         println!("Listening on {:?}", listen_addr);
    ///         let mut stream = tcp1.dial(listen_addr2.clone()).unwrap().await?;
    ///         // `stream` has `listen_addr1` as its local socket address.
    ///     }
    ///     _ => {}
    /// }
    ///
    /// let tcp2 = TcpConfig::new().port_reuse(true);
    /// let mut listener2 = tcp2.clone().listen_on(listen_addr2).expect("listener");
    /// match listener2.next().await.expect("event")? {
    ///     ListenerEvent::NewAddress(listen_addr) => {
    ///         println!("Listening on {:?}", listen_addr);
    ///         let mut socket = tcp2.dial(listen_addr1).unwrap().await?;
    ///         // `stream` has `listen_addr2` as its local socket address.
    ///     }
    ///     _ => {}
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// If a single `TcpConfig` is used and cloned for the creation of multiple
    /// listening sockets or a wildcard listen socket address is used to listen
    /// on any interface, there can be multiple such addresses registered for
    /// port reuse. In this case, one is chosen whose IP protocol version and
    /// loopback status is the same as that of the remote address. Consequently, for
    /// maximum control of the local listening addresses and ports that are used
    /// for outgoing connections, a new `TcpConfig` should be created for each
    /// listening socket, avoiding the use of wildcard addresses which bind a
    /// socket to all network interfaces.
    ///
    /// When this option is enabled on a unix system, the socket
    /// option `SO_REUSEPORT` is set, if available, to permit
    /// reuse of listening ports for multiple sockets.
    pub fn port_reuse(mut self, port_reuse: bool) -> Self {
        self.port_reuse = if port_reuse {
            PortReuse::Enabled {
                listen_addrs: Arc::new(RwLock::new(HashSet::new()))
            }
        } else {
            PortReuse::Disabled
        };

        self
    }

    fn create_socket(&self, socket_addr: &SocketAddr) -> io::Result<Socket> {
        let domain = if socket_addr.is_ipv4() {
            Domain::ipv4()
        } else {
            Domain::ipv6()
        };
        let socket = Socket::new(domain, Type::stream(), Some(socket2::Protocol::tcp()))?;
        if socket_addr.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        if let Some(ttl) = self.ttl {
            socket.set_ttl(ttl)?;
        }
        if let Some(nodelay) = self.nodelay {
            socket.set_nodelay(nodelay)?;
        }
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        if let PortReuse::Enabled { .. } = &self.port_reuse {
            socket.set_reuse_port(true)?;
        }
        Ok(socket)
    }

    fn do_listen(self, socket_addr: SocketAddr) -> io::Result<TcpListenStream> {
        let socket = self.create_socket(&socket_addr)?;
        socket.bind(&socket_addr.into())?;
        socket.listen(self.backlog as _)?;
        let listener = Async::new(socket.into_tcp_listener())?;
        TcpListenStream::new(listener, self.port_reuse)
    }

    async fn do_dial(self, socket_addr: SocketAddr) -> Result<Async<TcpStream>, io::Error> {
        let socket = self.create_socket(&socket_addr)?;

        if let Some(addr) = self.port_reuse.local_dial_addr(&socket_addr.ip()) {
            log::trace!("Binding dial socket to listen socket {}", addr);
            socket.bind(&addr.into())?;
        }

        socket.set_nonblocking(true)?;

        match socket.connect(&socket_addr.into()) {
            Ok(()) => {}
            Err(err) if err.raw_os_error() == Some(libc::EINPROGRESS) => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
            Err(err) => return Err(err),
        };
        let stream = Async::new(socket.into_tcp_stream())?;
        stream.writable().await?;
        Ok(stream)
    }
}

impl Transport for TcpConfig {
    type Output = Async<TcpStream>;
    type Error = io::Error;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Listener = TcpListenStream;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        log::debug!("listening on {}", socket_addr);
        self.do_listen(socket_addr)
            .map_err(|e| TransportError::Other(e))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                return Err(TransportError::MultiaddrNotSupported(addr));
            }
            socket_addr
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        log::debug!("dialing {}", socket_addr);
        Ok(Box::pin(self.do_dial(socket_addr)))
    }

    /// When port reuse is disabled and hence ephemeral local ports are
    /// used for outgoing connections, the returned address is the
    /// `observed` address with the port replaced by the port of the
    /// `listen` address.
    ///
    /// If port reuse is enabled, `Some(observed)` is returned, as there
    /// is a chance that the `observed` address _and_ port are reachable
    /// for other peers if there is a NAT in the way that does endpoint-
    /// independent filtering. Furthermore, even if that is not the case
    /// and TCP hole punching techniques must be used for NAT traversal,
    /// the `observed` address is still the one that a remote should connect
    /// to for the purpose of the hole punching procedure, as it represents
    /// the mapped IP and port of the NAT device in front of the local
    /// node.
    ///
    /// `None` is returned if one of the given addresses is not a TCP/IP
    /// address.
    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        match &self.port_reuse {
            PortReuse::Disabled => address_translation(listen, observed),
            PortReuse::Enabled { .. } => Some(observed.clone()),
        }
    }
}

type TcpListenerEvent = ListenerEvent<Ready<Result<Async<TcpStream>, io::Error>>, io::Error>;

enum IfWatch {
    Pending(BoxFuture<'static, io::Result<IfWatcher>>),
    Ready(IfWatcher),
}

/// The listening addresses of a [`TcpListenStream`].
enum InAddr {
    /// The stream accepts connections on a single interface.
    One {
        addr: IpAddr,
        out: Option<Multiaddr>
    },
    /// The stream accepts connections on all interfaces.
    Any {
        addrs: HashSet<IpAddr>,
        if_watch: IfWatch,
    }
}

/// A stream of incoming connections on one or more interfaces.
pub struct TcpListenStream {
    /// The socket address that the listening socket is bound to,
    /// which may be a "wildcard address" like `INADDR_ANY` or `IN6ADDR_ANY`
    /// when listening on all interfaces for IPv4 respectively IPv6 connections.
    listen_addr: SocketAddr,
    /// The async listening socket for incoming connections.
    listener: Async<TcpListener>,
    /// The IP addresses of network interfaces on which the listening socket
    /// is accepting connections.
    ///
    /// If the listen socket listens on all interfaces, these may change over
    /// time as interfaces become available or unavailable.
    in_addr: InAddr,
    /// The port reuse configuration for outgoing connections.
    ///
    /// If enabled, all IP addresses on which this listening stream
    /// is accepting connections (`in_addr`) are registered for reuse
    /// as local addresses for the sockets of outgoing connections. They are
    /// unregistered when the stream encounters an error or is dropped.
    port_reuse: PortReuse,
    /// How long to sleep after a (non-fatal) error while trying
    /// to accept a new connection.
    sleep_on_error: Duration,
    /// The current pause, if any.
    pause: Option<Timer>,
}

impl TcpListenStream {
    /// Constructs a `TcpListenStream` for incoming connections around
    /// the given `TcpListener`.
    fn new(listener: Async<TcpListener>, port_reuse: PortReuse)
        -> io::Result<Self>
    {
        let listen_addr = listener.get_ref().local_addr()?;

        let in_addr = if match &listen_addr {
            SocketAddr::V4(a) => a.ip().is_unspecified(),
            SocketAddr::V6(a) => a.ip().is_unspecified(),
        } {
            // The `addrs` are populated via `if_watch` when the
            // `TcpListenStream` is polled.
            InAddr::Any {
                addrs: HashSet::new(),
                if_watch: IfWatch::Pending(IfWatcher::new().boxed()),
            }
        } else {
            InAddr::One {
                out: Some(ip_to_multiaddr(listen_addr.ip(), listen_addr.port())),
                addr: listen_addr.ip(),
            }
        };

        Ok(TcpListenStream {
            port_reuse,
            listener,
            listen_addr,
            in_addr,
            pause: None,
            sleep_on_error: Duration::from_millis(100),
        })
    }

    /// Disables port reuse for any listen address of this stream.
    ///
    /// This is done when the `TcpListenStream` encounters a fatal
    /// error (for the stream) or is dropped.
    ///
    /// Has no effect if port reuse is disabled.
    fn disable_port_reuse(&mut self) {
        match &self.in_addr {
            InAddr::One { addr, .. } => {
                self.port_reuse.unregister(*addr, self.listen_addr.port());
            },
            InAddr::Any { addrs, .. } => {
                for addr in addrs {
                    self.port_reuse.unregister(*addr, self.listen_addr.port());
                }
            }
        }
    }
}

impl Drop for TcpListenStream {
    fn drop(&mut self) {
        self.disable_port_reuse();
    }
}

impl Stream for TcpListenStream {
    type Item = Result<TcpListenerEvent, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);

        loop {
            match &mut me.in_addr {
                InAddr::Any { if_watch, addrs } => match if_watch {
                    // If we listen on all interfaces, wait for `if-watch` to be ready.
                    IfWatch::Pending(f) => match ready!(Pin::new(f).poll(cx)) {
                        Ok(w) => {
                            *if_watch = IfWatch::Ready(w);
                            continue
                        }
                        Err(e) => {
                            me.disable_port_reuse();
                            return Poll::Ready(Some(Err(e)))
                        }
                    },
                    // Consume all events for up/down interface changes.
                    IfWatch::Ready(watch) => while let Some(ev) = watch.next().now_or_never() {
                        match ev {
                            Ok(IfEvent::Up(inet)) => {
                                let ip = inet.addr();
                                if me.listen_addr.is_ipv4() == ip.is_ipv4() {
                                    if addrs.insert(ip) {
                                        let ma = ip_to_multiaddr(ip, me.listen_addr.port());
                                        log::debug!("New listen address: {}", ma);
                                        me.port_reuse.register(ip, me.listen_addr.port());
                                        return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(ma))));
                                    }
                                }
                            }
                            Ok(IfEvent::Down(inet)) => {
                                let ip = inet.addr();
                                if me.listen_addr.is_ipv4() == ip.is_ipv4() {
                                    if addrs.remove(&ip) {
                                        let ma = ip_to_multiaddr(ip, me.listen_addr.port());
                                        log::debug!("Expired listen address: {}", ma);
                                        me.port_reuse.unregister(ip, me.listen_addr.port());
                                        return Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(ma))));
                                    }
                                }
                            }
                            Err(e) => {
                                me.disable_port_reuse();
                                return Poll::Ready(Some(Err(e)))
                            }
                        }
                    },
                },
                // If the listener is bound to a single interface, make sure the
                // address is registered for port reuse and reported once.
                InAddr::One { addr, out } => if let Some(multiaddr) = out.take() {
                    me.port_reuse.register(*addr, me.listen_addr.port());
                    return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(multiaddr))))
                }
            }

            if let Some(mut pause) = me.pause.take() {
                match Pin::new(&mut pause).poll(cx) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        me.pause = Some(pause);
                        return Poll::Pending;
                    }
                }
            }

            // Check if the listener is ready to accept a new connection.
            match me.listener.poll_readable(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(err)) => {
                    me.disable_port_reuse();
                    return Poll::Ready(Some(Err(err)))
                }
            }

            // Take the pending connection from the backlog.
            let (stream, sock_addr) = match me.listener.accept().now_or_never() {
                Some(Ok(res)) => res,
                Some(Err(e)) => {
                    // These errors are non-fatal for the listener stream.
                    log::error!("error accepting incoming connection: {}", e);
                    me.pause = Some(Timer::after(me.sleep_on_error));
                    return Poll::Ready(Some(Ok(ListenerEvent::Error(e))));
                }
                None => unreachable!("poll_readable() signaled readiness"),
            };

            let local_addr = match stream.get_ref().local_addr() {
                Ok(sock_addr) => ip_to_multiaddr(sock_addr.ip(), sock_addr.port()),
                Err(err) => {
                    log::error!("Failed to get local address of incoming socket: {:?}", err);
                    continue;
                }
            };
            let remote_addr = ip_to_multiaddr(sock_addr.ip(), sock_addr.port());

            log::debug!("Incoming connection from {} at {}", remote_addr, local_addr);

            return Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                upgrade: future::ok(stream),
                local_addr,
                remote_addr,
            })));
        }
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        return Err(());
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(()),
    }
}

// Create a [`Multiaddr`] from the given IP address and port number.
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    Multiaddr::empty()
        .with(ip.into())
        .with(Protocol::Tcp(port))
}

#[cfg(test)]
mod tests {
    use futures::channel::mpsc;
    use super::*;

    #[test]
    fn multiaddr_to_tcp_conversion() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert_eq!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/255.255.255.255/tcp/8080"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                8080,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip6/::1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/tcp/8080"
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
        env_logger::try_init().ok();

        fn test(addr: Multiaddr) {
            let (mut ready_tx, mut ready_rx) = mpsc::channel(1);

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new();
                let mut listener = tcp.listen_on(addr).unwrap();

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(listen_addr) => {
                            ready_tx.send(listen_addr).await.unwrap();
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
                let addr = ready_rx.next().await.unwrap();
                let tcp = TcpConfig::new();

                // Obtain a future socket through dialing
                let mut socket = tcp.dial(addr.clone()).unwrap().await.unwrap();
                socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

                let mut buf = [0u8; 3];
                socket.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [4, 5, 6]);
            });
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn wildcard_expansion() {
        env_logger::try_init().ok();

        fn test(addr: Multiaddr) {
            let (mut ready_tx, mut ready_rx) = mpsc::channel(1);

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new();
                let mut listener = tcp.listen_on(addr).unwrap();

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(a) => {
                            let mut iter = a.iter();
                            match iter.next().expect("ip address") {
                                Protocol::Ip4(ip) => assert!(!ip.is_unspecified()),
                                Protocol::Ip6(ip) => assert!(!ip.is_unspecified()),
                                other => panic!("Unexpected protocol: {}", other),
                            }
                            if let Protocol::Tcp(port) = iter.next().expect("port") {
                                assert_ne!(0, port)
                            } else {
                                panic!("No TCP port in address: {}", a)
                            }
                            ready_tx.send(a).await.ok();
                        }
                        _ => {}
                    }
                }
            });

            async_std::task::block_on(async move {
                let dest_addr = ready_rx.next().await.unwrap();
                let tcp = TcpConfig::new();
                tcp.dial(dest_addr).unwrap().await.unwrap();
            });
        }

        test("/ip4/0.0.0.0/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn port_reuse_dialing() {
        env_logger::try_init().ok();

        fn test(addr: Multiaddr) {
            let (mut ready_tx, mut ready_rx) = mpsc::channel(1);
            let addr2 = addr.clone();

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new();
                let mut listener = tcp.listen_on(addr).unwrap();

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(listen_addr) => {
                            ready_tx.send(listen_addr).await.ok();
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
                let dest_addr = ready_rx.next().await.unwrap();
                let tcp = TcpConfig::new().port_reuse(true);
                let mut listener = tcp.clone().listen_on(addr2).unwrap();
                match listener.next().await.unwrap().unwrap() {
                    ListenerEvent::NewAddress(_) => {
                        // Obtain a future socket through dialing
                        let mut socket = tcp.dial(dest_addr).unwrap().await.unwrap();
                        socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();
                        let mut buf = [0u8; 3];
                        socket.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, [4, 5, 6]);
                    }
                    e => panic!("Unexpected listener event: {:?}", e)
                }
            });
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn port_reuse_listening() {
        env_logger::try_init().ok();

        fn test(addr: Multiaddr) {
            let tcp = TcpConfig::new().port_reuse(true);

            async_std::task::spawn(async move {
                let mut listener1 = tcp.clone().listen_on(addr).unwrap();
                match listener1.next().await.unwrap().unwrap() {
                    ListenerEvent::NewAddress(addr1) => {
                        // Listen on the same address a second time.
                        let mut listener2 = tcp.clone().listen_on(addr1.clone()).unwrap();
                        match listener2.next().await.unwrap().unwrap() {
                            ListenerEvent::NewAddress(addr2) => {
                                assert_eq!(addr1, addr2);
                                return
                            }
                            e => panic!("Unexpected listener event: {:?}", e),
                        }
                    }
                    e => panic!("Unexpected listener event: {:?}", e),
                }
            });
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
    }

    #[async_std::test]
    async fn replace_port_0_in_returned_multiaddr_ipv4() {
        env_logger::try_init().ok();

        let tcp = TcpConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = tcp
            .listen_on(addr)
            .unwrap()
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[async_std::test]
    async fn replace_port_0_in_returned_multiaddr_ipv6() {
        env_logger::try_init().ok();

        let tcp = TcpConfig::new();

        let addr: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = tcp
            .listen_on(addr)
            .unwrap()
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[async_std::test]
    async fn larger_addr_denied() {
        env_logger::try_init().ok();

        let tcp = TcpConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(tcp.listen_on(addr).is_err());
    }
}
