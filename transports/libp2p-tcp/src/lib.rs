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
//! This crate provides a `TcpTransport` and `TokioTcpTransport`, depending on
//! the enabled features, which implement the `Transport` trait for use as a
//! transport with `libp2p-core` or `libp2p-swarm`.

mod provider;

use if_watch::{IfEvent, IfWatcher};
#[cfg(feature = "async-io")]
pub use provider::async_io;

/// The type of a [`GenTcpTransport`] using the `async-io` implementation.
#[cfg(feature = "async-io")]
pub type TcpTransport = GenTcpTransport<async_io::Tcp>;

#[cfg(feature = "tokio")]
pub use provider::tokio;

/// The type of a [`GenTcpTransport`] using the `tokio` implementation.
#[cfg(feature = "tokio")]
pub type TokioTcpTransport = GenTcpTransport<tokio::Tcp>;

use futures::{
    future::{self, Ready},
    prelude::*,
};
use futures_timer::Delay;
use libp2p_core::{
    address_translation,
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, Transport, TransportError, TransportEvent},
};
use socket2::{Domain, Socket, Type};
use std::{
    collections::{HashSet, VecDeque},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener},
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
    time::Duration,
};

use provider::{Incoming, Provider};

/// The configuration for a TCP/IP transport capability for libp2p.
#[derive(Clone, Debug)]
pub struct GenTcpConfig {
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
    /// Size of the listen backlog for listen sockets.
    backlog: u32,
    /// Whether port reuse should be enabled.
    enable_port_reuse: bool,
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
        listen_addrs: Arc<RwLock<HashSet<(IpAddr, Port)>>>,
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
                .expect("`local_dial_addr` never panic while holding the lock")
                .iter()
            {
                if ip.is_ipv4() == remote_ip.is_ipv4()
                    && ip.is_loopback() == remote_ip.is_loopback()
                {
                    if remote_ip.is_ipv4() {
                        return Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), *port));
                    } else {
                        return Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), *port));
                    }
                }
            }
        }

        None
    }
}

impl GenTcpConfig {
    /// Creates a new configuration for a TCP/IP transport:
    ///
    ///   * Nagle's algorithm, i.e. `TCP_NODELAY`, is _enabled_.
    ///     See [`GenTcpConfig::nodelay`].
    ///   * Reuse of listening ports is _disabled_.
    ///     See [`GenTcpConfig::port_reuse`].
    ///   * No custom `IP_TTL` is set. The default of the OS TCP stack applies.
    ///     See [`GenTcpConfig::ttl`].
    ///   * The size of the listen backlog for new listening sockets is `1024`.
    ///     See [`GenTcpConfig::listen_backlog`].
    pub fn new() -> Self {
        Self {
            ttl: None,
            nodelay: None,
            backlog: 1024,
            enable_port_reuse: false,
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
    /// [`GenTcpTransport`] keeps track of the listen socket addresses as they
    /// are reported by polling it. It is possible to listen on multiple
    /// addresses, enabling port reuse for each, knowing exactly which listen
    /// address is reused when dialing with a specific [`GenTcpTransport`], as in the
    /// following example:
    ///
    /// ```no_run
    /// # use futures::StreamExt;
    /// # use libp2p_core::transport::{ListenerId, TransportEvent};
    /// # use libp2p_core::{Multiaddr, Transport};
    /// # use std::pin::Pin;
    /// # #[cfg(not(feature = "async-io"))]
    /// # fn main() {}
    /// #
    /// #[cfg(feature = "async-io")]
    /// #[async_std::main]
    /// async fn main() -> std::io::Result<()> {
    /// use libp2p_tcp::{GenTcpConfig, TcpTransport};
    ///
    /// let listen_addr1: Multiaddr = "/ip4/127.0.0.1/tcp/9001".parse().unwrap();
    /// let listen_addr2: Multiaddr = "/ip4/127.0.0.1/tcp/9002".parse().unwrap();
    ///
    /// let mut tcp1 = TcpTransport::new(GenTcpConfig::new().port_reuse(true)).boxed();
    /// tcp1.listen_on( listen_addr1.clone()).expect("listener");
    /// match tcp1.select_next_some().await {
    ///     TransportEvent::NewAddress { listen_addr, .. } => {
    ///         println!("Listening on {:?}", listen_addr);
    ///         let mut stream = tcp1.dial(listen_addr2.clone()).unwrap().await?;
    ///         // `stream` has `listen_addr1` as its local socket address.
    ///     }
    ///     _ => {}
    /// }
    ///
    /// let mut tcp2 = TcpTransport::new(GenTcpConfig::new().port_reuse(true)).boxed();
    /// tcp2.listen_on( listen_addr2).expect("listener");
    /// match tcp2.select_next_some().await {
    ///     TransportEvent::NewAddress { listen_addr, .. } => {
    ///         println!("Listening on {:?}", listen_addr);
    ///         let mut socket = tcp2.dial(listen_addr1).unwrap().await?;
    ///         // `stream` has `listen_addr2` as its local socket address.
    ///     }
    ///     _ => {}
    /// }
    /// Ok(())
    /// }
    /// ```
    ///
    /// If a wildcard listen socket address is used to listen on any interface,
    /// there can be multiple such addresses registered for port reuse. In this
    /// case, one is chosen whose IP protocol version and loopback status is the
    /// same as that of the remote address. Consequently, for maximum control of
    /// the local listening addresses and ports that are used for outgoing
    /// connections, a new [`GenTcpTransport`] should be created for each listening
    /// socket, avoiding the use of wildcard addresses which bind a socket to
    /// all network interfaces.
    ///
    /// When this option is enabled on a unix system, the socket
    /// option `SO_REUSEPORT` is set, if available, to permit
    /// reuse of listening ports for multiple sockets.
    pub fn port_reuse(mut self, port_reuse: bool) -> Self {
        self.enable_port_reuse = port_reuse;
        self
    }
}

impl Default for GenTcpConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub struct GenTcpTransport<T>
where
    T: Provider + Send,
{
    config: GenTcpConfig,

    /// The configuration of port reuse when dialing.
    port_reuse: PortReuse,
    /// All the active listeners.
    /// The `TcpListenStream` struct contains a stream that we want to be pinned. Since the `VecDeque`
    /// can be resized, the only way is to use a `Pin<Box<>>`.
    listeners: VecDeque<Pin<Box<TcpListenStream<T>>>>,
    /// Pending transport events to return from [`GenTcpTransport::poll`].
    pending_events: VecDeque<TransportEvent<<Self as Transport>::ListenerUpgrade, io::Error>>,
}

impl<T> GenTcpTransport<T>
where
    T: Provider + Send,
{
    pub fn new(config: GenTcpConfig) -> Self {
        let port_reuse = if config.enable_port_reuse {
            PortReuse::Enabled {
                listen_addrs: Arc::new(RwLock::new(HashSet::new())),
            }
        } else {
            PortReuse::Disabled
        };
        GenTcpTransport {
            config,
            port_reuse,
            ..Default::default()
        }
    }

    fn create_socket(&self, socket_addr: &SocketAddr) -> io::Result<Socket> {
        let domain = if socket_addr.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };
        let socket = Socket::new(domain, Type::STREAM, Some(socket2::Protocol::TCP))?;
        if socket_addr.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        if let Some(ttl) = self.config.ttl {
            socket.set_ttl(ttl)?;
        }
        if let Some(nodelay) = self.config.nodelay {
            socket.set_nodelay(nodelay)?;
        }
        socket.set_reuse_address(true)?;
        #[cfg(unix)]
        if let PortReuse::Enabled { .. } = &self.port_reuse {
            socket.set_reuse_port(true)?;
        }
        Ok(socket)
    }

    fn do_listen(
        &mut self,
        id: ListenerId,
        socket_addr: SocketAddr,
    ) -> io::Result<TcpListenStream<T>> {
        let socket = self.create_socket(&socket_addr)?;
        socket.bind(&socket_addr.into())?;
        socket.listen(self.config.backlog as _)?;
        socket.set_nonblocking(true)?;
        let listener: TcpListener = socket.into();
        let local_addr = listener.local_addr()?;

        if local_addr.ip().is_unspecified() {
            return TcpListenStream::<T>::new(
                id,
                listener,
                Some(IfWatcher::new()?),
                self.port_reuse.clone(),
            );
        }

        self.port_reuse.register(local_addr.ip(), local_addr.port());
        let listen_addr = ip_to_multiaddr(local_addr.ip(), local_addr.port());
        self.pending_events.push_back(TransportEvent::NewAddress {
            listener_id: id,
            listen_addr,
        });
        TcpListenStream::<T>::new(id, listener, None, self.port_reuse.clone())
    }
}

impl<T> Default for GenTcpTransport<T>
where
    T: Provider + Send,
{
    fn default() -> Self {
        let config = GenTcpConfig::default();
        let port_reuse = if config.enable_port_reuse {
            PortReuse::Enabled {
                listen_addrs: Arc::new(RwLock::new(HashSet::new())),
            }
        } else {
            PortReuse::Disabled
        };
        GenTcpTransport {
            port_reuse,
            config,
            listeners: VecDeque::new(),
            pending_events: VecDeque::new(),
        }
    }
}

impl<T> Transport for GenTcpTransport<T>
where
    T: Provider + Send + 'static,
    T::Listener: Unpin,
    T::Stream: Unpin,
{
    type Output = T::Stream;
    type Error = io::Error;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(addr.clone()) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        let id = ListenerId::new();
        log::debug!("listening on {}", socket_addr);
        let listener = self
            .do_listen(id, socket_addr)
            .map_err(TransportError::Other)?;
        self.listeners.push_back(Box::pin(listener));
        Ok(id)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(index) = self.listeners.iter().position(|l| l.listener_id != id) {
            self.listeners.remove(index);
            self.pending_events
                .push_back(TransportEvent::ListenerClosed {
                    listener_id: id,
                    reason: Ok(()),
                });
            true
        } else {
            false
        }
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = if let Ok(socket_addr) = multiaddr_to_socketaddr(addr.clone()) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                return Err(TransportError::MultiaddrNotSupported(addr));
            }
            socket_addr
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        log::debug!("dialing {}", socket_addr);

        let socket = self
            .create_socket(&socket_addr)
            .map_err(TransportError::Other)?;

        if let Some(addr) = self.port_reuse.local_dial_addr(&socket_addr.ip()) {
            log::trace!("Binding dial socket to listen socket {}", addr);
            socket.bind(&addr.into()).map_err(TransportError::Other)?;
        }

        socket
            .set_nonblocking(true)
            .map_err(TransportError::Other)?;

        Ok(async move {
            // [`Transport::dial`] should do no work unless the returned [`Future`] is polled. Thus
            // do the `connect` call within the [`Future`].
            match socket.connect(&socket_addr.into()) {
                Ok(()) => {}
                Err(err) if err.raw_os_error() == Some(libc::EINPROGRESS) => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(err),
            };

            let stream = T::new_stream(socket.into()).await?;
            Ok(stream)
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.dial(addr)
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
        if !is_tcp_addr(listen) || !is_tcp_addr(observed) {
            return None;
        }
        match &self.port_reuse {
            PortReuse::Disabled => address_translation(listen, observed),
            PortReuse::Enabled { .. } => Some(observed.clone()),
        }
    }

    /// Poll all listeners.
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        // Return pending events from closed listeners.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        // We remove each element from `listeners` one by one and add them back.
        let mut remaining = self.listeners.len();
        while let Some(mut listener) = self.listeners.pop_back() {
            match TryStream::try_poll_next(listener.as_mut(), cx) {
                Poll::Pending => {
                    self.listeners.push_front(listener);
                    remaining -= 1;
                    if remaining == 0 {
                        break;
                    }
                }
                Poll::Ready(Some(Ok(TcpListenerEvent::Upgrade {
                    upgrade,
                    local_addr,
                    remote_addr,
                }))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::Incoming {
                        listener_id: id,
                        upgrade,
                        local_addr,
                        send_back_addr: remote_addr,
                    });
                }
                Poll::Ready(Some(Ok(TcpListenerEvent::NewAddress(a)))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::NewAddress {
                        listener_id: id,
                        listen_addr: a,
                    });
                }
                Poll::Ready(Some(Ok(TcpListenerEvent::AddressExpired(a)))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::AddressExpired {
                        listener_id: id,
                        listen_addr: a,
                    });
                }
                Poll::Ready(Some(Ok(TcpListenerEvent::Error(error)))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::ListenerError {
                        listener_id: id,
                        error,
                    });
                }
                Poll::Ready(None) => {
                    return Poll::Ready(TransportEvent::ListenerClosed {
                        listener_id: listener.listener_id,
                        reason: Ok(()),
                    });
                }
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(TransportEvent::ListenerClosed {
                        listener_id: listener.listener_id,
                        reason: Err(err),
                    });
                }
            }
        }
        Poll::Pending
    }
}

/// Event produced by a [`TcpListenStream`].
#[derive(Debug)]
pub enum TcpListenerEvent<S> {
    /// The listener is listening on a new additional [`Multiaddr`].
    NewAddress(Multiaddr),
    /// An upgrade, consisting of the upgrade future, the listener address and the remote address.
    Upgrade {
        /// The upgrade.
        upgrade: Ready<Result<S, io::Error>>,
        /// The local address which produced this upgrade.
        local_addr: Multiaddr,
        /// The remote address which produced this upgrade.
        remote_addr: Multiaddr,
    },
    /// A [`Multiaddr`] is no longer used for listening.
    AddressExpired(Multiaddr),
    /// A non-fatal error has happened on the listener.
    ///
    /// This event should be generated in order to notify the user that something wrong has
    /// happened. The listener, however, continues to run.
    Error(io::Error),
}

/// A stream of incoming connections on one or more interfaces.
pub struct TcpListenStream<T>
where
    T: Provider,
{
    /// The ID of this listener.
    listener_id: ListenerId,
    /// The socket address that the listening socket is bound to,
    /// which may be a "wildcard address" like `INADDR_ANY` or `IN6ADDR_ANY`
    /// when listening on all interfaces for IPv4 respectively IPv6 connections.
    listen_addr: SocketAddr,
    /// The async listening socket for incoming connections.
    listener: T::Listener,
    /// Watcher for network interface changes.
    /// Reports [`IfEvent`]s for new / deleted ip-addresses when interfaces
    /// become or stop being available.
    ///
    /// `None` if the socket is only listening on a single interface.
    if_watcher: Option<IfWatcher>,
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
    pause: Option<Delay>,
}

impl<T> TcpListenStream<T>
where
    T: Provider,
{
    /// Constructs a [`TcpListenStream`] for incoming connections around
    /// the given [`TcpListener`].
    fn new(
        listener_id: ListenerId,
        listener: TcpListener,
        if_watcher: Option<IfWatcher>,
        port_reuse: PortReuse,
    ) -> io::Result<Self> {
        let listen_addr = listener.local_addr()?;
        let listener = T::new_listener(listener)?;

        Ok(TcpListenStream {
            port_reuse,
            listener,
            listener_id,
            listen_addr,
            if_watcher,
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
        match &self.if_watcher {
            Some(if_watcher) => {
                for ip_net in if_watcher.iter() {
                    self.port_reuse
                        .unregister(ip_net.addr(), self.listen_addr.port());
                }
            }
            None => self
                .port_reuse
                .unregister(self.listen_addr.ip(), self.listen_addr.port()),
        }
    }
}

impl<T> Drop for TcpListenStream<T>
where
    T: Provider,
{
    fn drop(&mut self) {
        self.disable_port_reuse();
    }
}

impl<T> Stream for TcpListenStream<T>
where
    T: Provider,
    T::Listener: Unpin,
    T::Stream: Unpin,
{
    type Item = Result<TcpListenerEvent<T::Stream>, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);

        if let Some(mut pause) = me.pause.take() {
            match pause.poll_unpin(cx) {
                Poll::Ready(_) => {}
                Poll::Pending => {
                    me.pause = Some(pause);
                    return Poll::Pending;
                }
            }
        }

        if let Some(if_watcher) = me.if_watcher.as_mut() {
            while let Poll::Ready(event) = if_watcher.poll_if_event(cx) {
                match event {
                    Ok(IfEvent::Up(inet)) => {
                        let ip = inet.addr();
                        if me.listen_addr.is_ipv4() == ip.is_ipv4() {
                            let ma = ip_to_multiaddr(ip, me.listen_addr.port());
                            log::debug!("New listen address: {}", ma);
                            me.port_reuse.register(ip, me.listen_addr.port());
                            return Poll::Ready(Some(Ok(TcpListenerEvent::NewAddress(ma))));
                        }
                    }
                    Ok(IfEvent::Down(inet)) => {
                        let ip = inet.addr();
                        if me.listen_addr.is_ipv4() == ip.is_ipv4() {
                            let ma = ip_to_multiaddr(ip, me.listen_addr.port());
                            log::debug!("Expired listen address: {}", ma);
                            me.port_reuse.unregister(ip, me.listen_addr.port());
                            return Poll::Ready(Some(Ok(TcpListenerEvent::AddressExpired(ma))));
                        }
                    }
                    Err(err) => {
                        me.pause = Some(Delay::new(me.sleep_on_error));
                        return Poll::Ready(Some(Ok(TcpListenerEvent::Error(err))));
                    }
                }
            }
        }

        // Take the pending connection from the backlog.
        match T::poll_accept(&mut me.listener, cx) {
            Poll::Ready(Ok(Incoming {
                local_addr,
                remote_addr,
                stream,
            })) => {
                let local_addr = ip_to_multiaddr(local_addr.ip(), local_addr.port());
                let remote_addr = ip_to_multiaddr(remote_addr.ip(), remote_addr.port());

                log::debug!("Incoming connection from {} at {}", remote_addr, local_addr);

                return Poll::Ready(Some(Ok(TcpListenerEvent::Upgrade {
                    upgrade: future::ok(stream),
                    local_addr,
                    remote_addr,
                })));
            }
            Poll::Ready(Err(e)) => {
                // These errors are non-fatal for the listener stream.
                me.pause = Some(Delay::new(me.sleep_on_error));
                return Poll::Ready(Some(Ok(TcpListenerEvent::Error(e))));
            }
            Poll::Pending => {}
        };

        Poll::Pending
    }
}

/// Extracts a `SocketAddr` from a given `Multiaddr`.
///
/// Fails if the given `Multiaddr` does not begin with an IP
/// protocol encapsulating a TCP port.
fn multiaddr_to_socketaddr(mut addr: Multiaddr) -> Result<SocketAddr, ()> {
    // "Pop" the IP address and TCP port from the end of the address,
    // ignoring a `/p2p/...` suffix as well as any prefix of possibly
    // outer protocols, if present.
    let mut port = None;
    while let Some(proto) = addr.pop() {
        match proto {
            Protocol::Ip4(ipv4) => match port {
                Some(port) => return Ok(SocketAddr::new(ipv4.into(), port)),
                None => return Err(()),
            },
            Protocol::Ip6(ipv6) => match port {
                Some(port) => return Ok(SocketAddr::new(ipv6.into(), port)),
                None => return Err(()),
            },
            Protocol::Tcp(portnum) => match port {
                Some(_) => return Err(()),
                None => port = Some(portnum),
            },
            Protocol::P2p(_) => {}
            _ => return Err(()),
        }
    }
    Err(())
}

// Create a [`Multiaddr`] from the given IP address and port number.
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    Multiaddr::empty().with(ip.into()).with(Protocol::Tcp(port))
}

fn is_tcp_addr(addr: &Multiaddr) -> bool {
    use Protocol::*;

    let mut iter = addr.iter();

    let first = match iter.next() {
        None => return false,
        Some(p) => p,
    };
    let second = match iter.next() {
        None => return false,
        Some(p) => p,
    };

    matches!(first, Ip4(_) | Ip6(_) | Dns(_) | Dns4(_) | Dns6(_)) && matches!(second, Tcp(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{
        channel::{mpsc, oneshot},
        future::poll_fn,
    };
    use libp2p_core::PeerId;

    #[test]
    fn multiaddr_to_tcp_conversion() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

        assert!(
            multiaddr_to_socketaddr("/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert_eq!(
            multiaddr_to_socketaddr("/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                "/ip4/255.255.255.255/tcp/8080"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                8080,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr("/ip6/::1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                "/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/tcp/8080"
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

        async fn listener<T: Provider>(addr: Multiaddr, mut ready_tx: mpsc::Sender<Multiaddr>) {
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new()).boxed();
            tcp.listen_on(addr).unwrap();
            loop {
                match tcp.select_next_some().await {
                    TransportEvent::NewAddress { listen_addr, .. } => {
                        ready_tx.send(listen_addr).await.unwrap();
                    }
                    TransportEvent::Incoming { upgrade, .. } => {
                        let mut upgrade = upgrade.await.unwrap();
                        let mut buf = [0u8; 3];
                        upgrade.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, [1, 2, 3]);
                        upgrade.write_all(&[4, 5, 6]).await.unwrap();
                        return;
                    }
                    e => panic!("Unexpected transport event: {:?}", e),
                }
            }
        }

        async fn dialer<T: Provider>(mut ready_rx: mpsc::Receiver<Multiaddr>) {
            let addr = ready_rx.next().await.unwrap();
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new());

            // Obtain a future socket through dialing
            let mut socket = tcp.dial(addr.clone()).unwrap().await.unwrap();
            socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

            let mut buf = [0u8; 3];
            socket.read_exact(&mut buf).await.unwrap();
            assert_eq!(buf, [4, 5, 6]);
        }

        fn test(addr: Multiaddr) {
            #[cfg(feature = "async-io")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let listener = listener::<async_io::Tcp>(addr.clone(), ready_tx);
                let dialer = dialer::<async_io::Tcp>(ready_rx);
                let listener = async_std::task::spawn(listener);
                async_std::task::block_on(dialer);
                async_std::task::block_on(listener);
            }

            #[cfg(feature = "tokio")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let listener = listener::<tokio::Tcp>(addr, ready_tx);
                let dialer = dialer::<tokio::Tcp>(ready_rx);
                let rt = tokio_crate::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap();
                let tasks = tokio_crate::task::LocalSet::new();
                let listener = tasks.spawn_local(listener);
                tasks.block_on(&rt, dialer);
                tasks.block_on(&rt, listener).unwrap();
            }
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn wildcard_expansion() {
        env_logger::try_init().ok();

        async fn listener<T: Provider>(addr: Multiaddr, mut ready_tx: mpsc::Sender<Multiaddr>) {
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new()).boxed();
            tcp.listen_on(addr).unwrap();

            loop {
                match tcp.select_next_some().await {
                    TransportEvent::NewAddress { listen_addr, .. } => {
                        let mut iter = listen_addr.iter();
                        match iter.next().expect("ip address") {
                            Protocol::Ip4(ip) => assert!(!ip.is_unspecified()),
                            Protocol::Ip6(ip) => assert!(!ip.is_unspecified()),
                            other => panic!("Unexpected protocol: {}", other),
                        }
                        if let Protocol::Tcp(port) = iter.next().expect("port") {
                            assert_ne!(0, port)
                        } else {
                            panic!("No TCP port in address: {}", listen_addr)
                        }
                        ready_tx.send(listen_addr).await.ok();
                    }
                    TransportEvent::Incoming { .. } => {
                        return;
                    }
                    _ => {}
                }
            }
        }

        async fn dialer<T: Provider>(mut ready_rx: mpsc::Receiver<Multiaddr>) {
            let dest_addr = ready_rx.next().await.unwrap();
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new());
            tcp.dial(dest_addr).unwrap().await.unwrap();
        }

        fn test(addr: Multiaddr) {
            #[cfg(feature = "async-io")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let listener = listener::<async_io::Tcp>(addr.clone(), ready_tx);
                let dialer = dialer::<async_io::Tcp>(ready_rx);
                let listener = async_std::task::spawn(listener);
                async_std::task::block_on(dialer);
                async_std::task::block_on(listener);
            }

            #[cfg(feature = "tokio")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let listener = listener::<tokio::Tcp>(addr, ready_tx);
                let dialer = dialer::<tokio::Tcp>(ready_rx);
                let rt = tokio_crate::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap();
                let tasks = tokio_crate::task::LocalSet::new();
                let listener = tasks.spawn_local(listener);
                tasks.block_on(&rt, dialer);
                tasks.block_on(&rt, listener).unwrap();
            }
        }

        test("/ip4/0.0.0.0/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn port_reuse_dialing() {
        env_logger::try_init().ok();

        async fn listener<T: Provider>(
            addr: Multiaddr,
            mut ready_tx: mpsc::Sender<Multiaddr>,
            port_reuse_rx: oneshot::Receiver<Protocol<'_>>,
        ) {
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new()).boxed();
            tcp.listen_on(addr).unwrap();
            loop {
                match tcp.select_next_some().await {
                    TransportEvent::NewAddress { listen_addr, .. } => {
                        ready_tx.send(listen_addr).await.ok();
                    }
                    TransportEvent::Incoming {
                        upgrade,
                        mut send_back_addr,
                        ..
                    } => {
                        // Receive the dialer tcp port reuse
                        let remote_port_reuse = port_reuse_rx.await.unwrap();
                        // And check it is the same as the remote port used for upgrade
                        assert_eq!(send_back_addr.pop().unwrap(), remote_port_reuse);

                        let mut upgrade = upgrade.await.unwrap();
                        let mut buf = [0u8; 3];
                        upgrade.read_exact(&mut buf).await.unwrap();
                        assert_eq!(buf, [1, 2, 3]);
                        upgrade.write_all(&[4, 5, 6]).await.unwrap();
                        return;
                    }
                    e => panic!("Unexpected event: {:?}", e),
                }
            }
        }

        async fn dialer<T: Provider>(
            addr: Multiaddr,
            mut ready_rx: mpsc::Receiver<Multiaddr>,
            port_reuse_tx: oneshot::Sender<Protocol<'_>>,
        ) {
            let dest_addr = ready_rx.next().await.unwrap();
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new().port_reuse(true));
            tcp.listen_on(addr).unwrap();
            match poll_fn(|cx| Pin::new(&mut tcp).poll(cx)).await {
                TransportEvent::NewAddress { .. } => {
                    // Check that tcp and listener share the same port reuse SocketAddr
                    let listener = tcp.listeners.front().unwrap();
                    let port_reuse_tcp = tcp.port_reuse.local_dial_addr(&listener.listen_addr.ip());
                    let port_reuse_listener = listener
                        .port_reuse
                        .local_dial_addr(&listener.listen_addr.ip());
                    assert!(port_reuse_tcp.is_some());
                    assert_eq!(port_reuse_tcp, port_reuse_listener);

                    // Send the dialer tcp port reuse to the listener
                    port_reuse_tx
                        .send(Protocol::Tcp(port_reuse_tcp.unwrap().port()))
                        .ok();

                    // Obtain a future socket through dialing
                    let mut socket = tcp.dial(dest_addr).unwrap().await.unwrap();
                    socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();
                    // socket.flush().await;
                    let mut buf = [0u8; 3];
                    socket.read_exact(&mut buf).await.unwrap();
                    assert_eq!(buf, [4, 5, 6]);
                }
                e => panic!("Unexpected transport event: {:?}", e),
            }
        }

        fn test(addr: Multiaddr) {
            #[cfg(feature = "async-io")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let (port_reuse_tx, port_reuse_rx) = oneshot::channel();
                let listener = listener::<async_io::Tcp>(addr.clone(), ready_tx, port_reuse_rx);
                let dialer = dialer::<async_io::Tcp>(addr.clone(), ready_rx, port_reuse_tx);
                let listener = async_std::task::spawn(listener);
                async_std::task::block_on(dialer);
                async_std::task::block_on(listener);
            }

            #[cfg(feature = "tokio")]
            {
                let (ready_tx, ready_rx) = mpsc::channel(1);
                let (port_reuse_tx, port_reuse_rx) = oneshot::channel();
                let listener = listener::<tokio::Tcp>(addr.clone(), ready_tx, port_reuse_rx);
                let dialer = dialer::<tokio::Tcp>(addr, ready_rx, port_reuse_tx);
                let rt = tokio_crate::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap();
                let tasks = tokio_crate::task::LocalSet::new();
                let listener = tasks.spawn_local(listener);
                tasks.block_on(&rt, dialer);
                tasks.block_on(&rt, listener).unwrap();
            }
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn port_reuse_listening() {
        env_logger::try_init().ok();

        async fn listen_twice<T: Provider>(addr: Multiaddr) {
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new().port_reuse(true));
            tcp.listen_on(addr).unwrap();
            match poll_fn(|cx| Pin::new(&mut tcp).poll(cx)).await {
                TransportEvent::NewAddress {
                    listen_addr: addr1, ..
                } => {
                    let listener1 = tcp.listeners.front().unwrap();
                    let port_reuse_tcp =
                        tcp.port_reuse.local_dial_addr(&listener1.listen_addr.ip());
                    let port_reuse_listener1 = listener1
                        .port_reuse
                        .local_dial_addr(&listener1.listen_addr.ip());
                    assert!(port_reuse_tcp.is_some());
                    assert_eq!(port_reuse_tcp, port_reuse_listener1);

                    // Listen on the same address a second time.
                    tcp.listen_on(addr1.clone()).unwrap();
                    match poll_fn(|cx| Pin::new(&mut tcp).poll(cx)).await {
                        TransportEvent::NewAddress {
                            listen_addr: addr2, ..
                        } => assert_eq!(addr1, addr2),
                        e => panic!("Unexpected transport event: {:?}", e),
                    }
                }
                e => panic!("Unexpected transport event: {:?}", e),
            }
        }

        fn test(addr: Multiaddr) {
            #[cfg(feature = "async-io")]
            {
                let listener = listen_twice::<async_io::Tcp>(addr.clone());
                async_std::task::block_on(listener);
            }

            #[cfg(feature = "tokio")]
            {
                let listener = listen_twice::<tokio::Tcp>(addr);
                let rt = tokio_crate::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap();
                rt.block_on(listener);
            }
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
    }

    #[test]
    fn listen_port_0() {
        env_logger::try_init().ok();

        async fn listen<T: Provider>(addr: Multiaddr) -> Multiaddr {
            let mut tcp = GenTcpTransport::<T>::new(GenTcpConfig::new()).boxed();
            tcp.listen_on(addr).unwrap();
            tcp.select_next_some()
                .await
                .into_new_address()
                .expect("listen address")
        }

        fn test(addr: Multiaddr) {
            #[cfg(feature = "async-io")]
            {
                let new_addr = async_std::task::block_on(listen::<async_io::Tcp>(addr.clone()));
                assert!(!new_addr.to_string().contains("tcp/0"));
            }

            #[cfg(feature = "tokio")]
            {
                let rt = tokio_crate::runtime::Builder::new_current_thread()
                    .enable_io()
                    .build()
                    .unwrap();
                let new_addr = rt.block_on(listen::<tokio::Tcp>(addr));
                assert!(!new_addr.to_string().contains("tcp/0"));
            }
        }

        test("/ip6/::1/tcp/0".parse().unwrap());
        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
    }

    #[test]
    fn listen_invalid_addr() {
        env_logger::try_init().ok();

        fn test(addr: Multiaddr) {
            #[cfg(feature = "async-io")]
            {
                let mut tcp = TcpTransport::new(GenTcpConfig::new());
                assert!(tcp.listen_on(addr.clone()).is_err());
            }

            #[cfg(feature = "tokio")]
            {
                let mut tcp = TokioTcpTransport::new(GenTcpConfig::new());
                assert!(tcp.listen_on(addr).is_err());
            }
        }

        test("/ip4/127.0.0.1/tcp/12345/tcp/12345".parse().unwrap());
    }

    #[cfg(any(feature = "async-io", feature = "tcp"))]
    #[test]
    fn test_address_translation() {
        #[cfg(feature = "async-io")]
        let transport = TcpTransport::new(GenTcpConfig::new());
        #[cfg(all(feature = "tokio", not(feature = "async-io")))]
        let transport = TokioTcpTransport::new(GenTcpConfig::new());

        let port = 42;
        let tcp_listen_addr = Multiaddr::empty()
            .with(Protocol::Ip4(Ipv4Addr::new(127, 0, 0, 1)))
            .with(Protocol::Tcp(port));
        let observed_ip = Ipv4Addr::new(123, 45, 67, 8);
        let tcp_observed_addr = Multiaddr::empty()
            .with(Protocol::Ip4(observed_ip))
            .with(Protocol::Tcp(1))
            .with(Protocol::P2p(PeerId::random().into()));

        let translated = transport
            .address_translation(&tcp_listen_addr, &tcp_observed_addr)
            .unwrap();
        let mut iter = translated.iter();
        assert_eq!(iter.next(), Some(Protocol::Ip4(observed_ip)));
        assert_eq!(iter.next(), Some(Protocol::Tcp(port)));
        assert_eq!(iter.next(), None);

        let quic_addr = Multiaddr::empty()
            .with(Protocol::Ip4(Ipv4Addr::new(87, 65, 43, 21)))
            .with(Protocol::Udp(1))
            .with(Protocol::Quic);

        assert!(transport
            .address_translation(&tcp_listen_addr, &quic_addr)
            .is_none());
        assert!(transport
            .address_translation(&quic_addr, &tcp_observed_addr)
            .is_none());
    }
}
