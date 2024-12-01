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

use std::{
    collections::{
        hash_map::{DefaultHasher, Entry},
        HashMap, HashSet,
    },
    fmt,
    hash::{Hash, Hasher},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::{
    channel::oneshot,
    future::{BoxFuture, Either},
    prelude::*,
    ready,
    stream::{SelectAll, StreamExt},
};
use if_watch::IfEvent;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{DialOpts, ListenerId, PortUse, TransportError, TransportEvent},
    Endpoint, Transport,
};
use libp2p_identity::PeerId;
use socket2::{Domain, Socket, Type};

use crate::{
    config::{Config, QuinnConfig},
    hole_punching::hole_puncher,
    provider::Provider,
    ConnectError, Connecting, Connection, Error,
};

/// Implementation of the [`Transport`] trait for QUIC.
///
/// By default only QUIC Version 1 (RFC 9000) is supported. In the [`Multiaddr`] this maps to
/// [`libp2p_core::multiaddr::Protocol::QuicV1`].
/// The [`libp2p_core::multiaddr::Protocol::Quic`] codepoint is interpreted as QUIC version
/// draft-29 and only supported if [`Config::support_draft_29`] is set to `true`.
/// Note that in that case servers support both version an all QUIC listening addresses.
///
/// Version draft-29 should only be used to connect to nodes from other libp2p implementations
/// that do not support `QuicV1` yet. Support for it will be removed long-term.
/// See <https://github.com/multiformats/multiaddr/issues/145>.
#[derive(Debug)]
pub struct GenTransport<P: Provider> {
    /// Config for the inner [`quinn`] structs.
    quinn_config: QuinnConfig,
    /// Timeout for the [`Connecting`] future.
    handshake_timeout: Duration,
    /// Whether draft-29 is supported for dialing and listening.
    support_draft_29: bool,
    /// Streams of active [`Listener`]s.
    listeners: SelectAll<Listener<P>>,
    /// Dialer for each socket family if no matching listener exists.
    dialer: HashMap<SocketFamily, quinn::Endpoint>,
    /// Waker to poll the transport again when a new dialer or listener is added.
    waker: Option<Waker>,
    /// Holepunching attempts
    hole_punch_attempts: HashMap<SocketAddr, oneshot::Sender<Connecting>>,
}

impl<P: Provider> GenTransport<P> {
    /// Create a new [`GenTransport`] with the given [`Config`].
    pub fn new(config: Config) -> Self {
        let handshake_timeout = config.handshake_timeout;
        let support_draft_29 = config.support_draft_29;
        let quinn_config = config.into();
        Self {
            listeners: SelectAll::new(),
            quinn_config,
            handshake_timeout,
            dialer: HashMap::new(),
            waker: None,
            support_draft_29,
            hole_punch_attempts: Default::default(),
        }
    }

    /// Create a new [`quinn::Endpoint`] with the given configs.
    fn new_endpoint(
        endpoint_config: quinn::EndpointConfig,
        server_config: Option<quinn::ServerConfig>,
        socket: UdpSocket,
    ) -> Result<quinn::Endpoint, Error> {
        use crate::provider::Runtime;
        match P::runtime() {
            #[cfg(feature = "tokio")]
            Runtime::Tokio => {
                let runtime = std::sync::Arc::new(quinn::TokioRuntime);
                let endpoint =
                    quinn::Endpoint::new(endpoint_config, server_config, socket, runtime)?;
                Ok(endpoint)
            }
            #[cfg(feature = "async-std")]
            Runtime::AsyncStd => {
                let runtime = std::sync::Arc::new(quinn::AsyncStdRuntime);
                let endpoint =
                    quinn::Endpoint::new(endpoint_config, server_config, socket, runtime)?;
                Ok(endpoint)
            }
            Runtime::Dummy => {
                let _ = endpoint_config;
                let _ = server_config;
                let _ = socket;
                let err = std::io::Error::new(std::io::ErrorKind::Other, "no async runtime found");
                Err(Error::Io(err))
            }
        }
    }

    /// Extract the addr, quic version and peer id from the given [`Multiaddr`].
    fn remote_multiaddr_to_socketaddr(
        &self,
        addr: Multiaddr,
        check_unspecified_addr: bool,
    ) -> Result<
        (SocketAddr, ProtocolVersion, Option<PeerId>),
        TransportError<<Self as Transport>::Error>,
    > {
        let (socket_addr, version, peer_id) = multiaddr_to_socketaddr(&addr, self.support_draft_29)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if check_unspecified_addr && (socket_addr.port() == 0 || socket_addr.ip().is_unspecified())
        {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        Ok((socket_addr, version, peer_id))
    }

    /// Pick any listener to use for dialing.
    fn eligible_listener(&mut self, socket_addr: &SocketAddr) -> Option<&mut Listener<P>> {
        let mut listeners: Vec<_> = self
            .listeners
            .iter_mut()
            .filter(|l| {
                if l.is_closed {
                    return false;
                }
                SocketFamily::is_same(&l.socket_addr().ip(), &socket_addr.ip())
            })
            .filter(|l| {
                if socket_addr.ip().is_loopback() {
                    l.listening_addresses
                        .iter()
                        .any(|ip_addr| ip_addr.is_loopback())
                } else {
                    true
                }
            })
            .collect();
        match listeners.len() {
            0 => None,
            1 => listeners.pop(),
            _ => {
                // Pick any listener to use for dialing.
                // We hash the socket address to achieve determinism.
                let mut hasher = DefaultHasher::new();
                socket_addr.hash(&mut hasher);
                let index = hasher.finish() as usize % listeners.len();
                Some(listeners.swap_remove(index))
            }
        }
    }

    fn create_socket(&self, socket_addr: SocketAddr) -> io::Result<UdpSocket> {
        let socket = Socket::new(
            Domain::for_address(socket_addr),
            Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;
        if socket_addr.is_ipv6() {
            socket.set_only_v6(true)?;
        }

        socket.bind(&socket_addr.into())?;

        Ok(socket.into())
    }

    fn bound_socket(&mut self, socket_addr: SocketAddr) -> Result<quinn::Endpoint, Error> {
        let socket_family = socket_addr.ip().into();
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        let listen_socket_addr = match socket_family {
            SocketFamily::Ipv4 => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
            SocketFamily::Ipv6 => SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0),
        };
        let socket = UdpSocket::bind(listen_socket_addr)?;
        let endpoint_config = self.quinn_config.endpoint_config.clone();
        let endpoint = Self::new_endpoint(endpoint_config, None, socket)?;
        Ok(endpoint)
    }
}

impl<P: Provider> Transport for GenTransport<P> {
    type Output = (PeerId, Connection);
    type Error = Error;
    type ListenerUpgrade = Connecting;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        listener_id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        let (socket_addr, version, _peer_id) = self.remote_multiaddr_to_socketaddr(addr, false)?;
        let endpoint_config = self.quinn_config.endpoint_config.clone();
        let server_config = self.quinn_config.server_config.clone();
        let socket = self.create_socket(socket_addr).map_err(Self::Error::from)?;

        let socket_c = socket.try_clone().map_err(Self::Error::from)?;
        let endpoint = Self::new_endpoint(endpoint_config, Some(server_config), socket)?;
        let listener = Listener::new(
            listener_id,
            socket_c,
            endpoint,
            self.handshake_timeout,
            version,
        )?;
        self.listeners.push(listener);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        // Remove dialer endpoint so that the endpoint is dropped once the last
        // connection that uses it is closed.
        // New outbound connections will use the bidirectional (listener) endpoint.
        self.dialer.remove(&socket_addr.ip().into());

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(listener) = self.listeners.iter_mut().find(|l| l.listener_id == id) {
            // Close the listener, which will eventually finish its stream.
            // `SelectAll` removes streams once they are finished.
            listener.close(Ok(()));
            true
        } else {
            false
        }
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (socket_addr, version, peer_id) =
            self.remote_multiaddr_to_socketaddr(addr.clone(), true)?;

        match (dial_opts.role, dial_opts.port_use) {
            (Endpoint::Dialer, _) | (Endpoint::Listener, PortUse::Reuse) => {
                let endpoint = if let Some(listener) = dial_opts
                    .port_use
                    .eq(&PortUse::Reuse)
                    .then(|| self.eligible_listener(&socket_addr))
                    .flatten()
                {
                    listener.endpoint.clone()
                } else {
                    let socket_family = socket_addr.ip().into();
                    let dialer = if dial_opts.port_use == PortUse::Reuse {
                        if let Some(occupied) = self.dialer.get(&socket_family) {
                            occupied.clone()
                        } else {
                            let endpoint = self.bound_socket(socket_addr)?;
                            self.dialer.insert(socket_family, endpoint.clone());
                            endpoint
                        }
                    } else {
                        self.bound_socket(socket_addr)?
                    };
                    dialer
                };
                let handshake_timeout = self.handshake_timeout;
                let mut client_config = self.quinn_config.client_config.clone();
                if version == ProtocolVersion::Draft29 {
                    client_config.version(0xff00_001d);
                }
                Ok(Box::pin(async move {
                    // This `"l"` seems necessary because an empty string is an invalid domain
                    // name. While we don't use domain names, the underlying rustls library
                    // is based upon the assumption that we do.
                    let connecting = endpoint
                        .connect_with(client_config, socket_addr, "l")
                        .map_err(ConnectError)?;
                    Connecting::new(connecting, handshake_timeout).await
                }))
            }
            (Endpoint::Listener, _) => {
                let peer_id = peer_id.ok_or(TransportError::MultiaddrNotSupported(addr.clone()))?;

                let socket = self
                    .eligible_listener(&socket_addr)
                    .ok_or(TransportError::Other(
                        Error::NoActiveListenerForDialAsListener,
                    ))?
                    .try_clone_socket()
                    .map_err(Self::Error::from)?;

                tracing::debug!("Preparing for hole-punch from {addr}");

                let hole_puncher = hole_puncher::<P>(socket, socket_addr, self.handshake_timeout);

                let (sender, receiver) = oneshot::channel();

                match self.hole_punch_attempts.entry(socket_addr) {
                    Entry::Occupied(mut sender_entry) => {
                        // Stale senders, i.e. from failed hole punches are not removed.
                        // Thus, we can just overwrite a stale sender.
                        if !sender_entry.get().is_canceled() {
                            return Err(TransportError::Other(Error::HolePunchInProgress(
                                socket_addr,
                            )));
                        }
                        sender_entry.insert(sender);
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(sender);
                    }
                };

                Ok(Box::pin(async move {
                    futures::pin_mut!(hole_puncher);
                    match futures::future::select(receiver, hole_puncher).await {
                        Either::Left((message, _)) => {
                            let (inbound_peer_id, connection) = message
                                .expect(
                                    "hole punch connection sender is never dropped before receiver",
                                )
                                .await?;
                            if inbound_peer_id != peer_id {
                                tracing::warn!(
                                    peer=%peer_id,
                                    inbound_peer=%inbound_peer_id,
                                    socket_address=%socket_addr,
                                    "expected inbound connection from socket_address to resolve to peer but got inbound peer"
                                );
                            }
                            Ok((inbound_peer_id, connection))
                        }
                        Either::Right((hole_punch_err, _)) => Err(hole_punch_err),
                    }
                }))
            }
        }
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        while let Poll::Ready(Some(ev)) = self.listeners.poll_next_unpin(cx) {
            match ev {
                TransportEvent::Incoming {
                    listener_id,
                    mut upgrade,
                    local_addr,
                    send_back_addr,
                } => {
                    let socket_addr =
                        multiaddr_to_socketaddr(&send_back_addr, self.support_draft_29)
                            .unwrap()
                            .0;

                    if let Some(sender) = self.hole_punch_attempts.remove(&socket_addr) {
                        match sender.send(upgrade) {
                            Ok(()) => continue,
                            Err(timed_out_holepunch) => {
                                upgrade = timed_out_holepunch;
                            }
                        }
                    }

                    return Poll::Ready(TransportEvent::Incoming {
                        listener_id,
                        upgrade,
                        local_addr,
                        send_back_addr,
                    });
                }
                _ => return Poll::Ready(ev),
            }
        }

        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl From<Error> for TransportError<Error> {
    fn from(err: Error) -> Self {
        TransportError::Other(err)
    }
}

/// Listener for incoming connections.
struct Listener<P: Provider> {
    /// Id of the listener.
    listener_id: ListenerId,

    /// Version of the supported quic protocol.
    version: ProtocolVersion,

    /// Endpoint
    endpoint: quinn::Endpoint,

    /// An underlying copy of the socket to be able to hole punch with
    socket: UdpSocket,

    /// A future to poll new incoming connections.
    accept: BoxFuture<'static, Option<quinn::Incoming>>,
    /// Timeout for connection establishment on inbound connections.
    handshake_timeout: Duration,

    /// Watcher for network interface changes.
    ///
    /// None if we are only listening on a single interface.
    if_watcher: Option<P::IfWatcher>,

    /// Whether the listener was closed and the stream should terminate.
    is_closed: bool,

    /// Pending event to reported.
    pending_event: Option<<Self as Stream>::Item>,

    /// The stream must be awaken after it has been closed to deliver the last event.
    close_listener_waker: Option<Waker>,

    listening_addresses: HashSet<IpAddr>,
}

impl<P: Provider> Listener<P> {
    fn new(
        listener_id: ListenerId,
        socket: UdpSocket,
        endpoint: quinn::Endpoint,
        handshake_timeout: Duration,
        version: ProtocolVersion,
    ) -> Result<Self, Error> {
        let if_watcher;
        let pending_event;
        let mut listening_addresses = HashSet::new();
        let local_addr = socket.local_addr()?;
        if local_addr.ip().is_unspecified() {
            if_watcher = Some(P::new_if_watcher()?);
            pending_event = None;
        } else {
            if_watcher = None;
            listening_addresses.insert(local_addr.ip());
            let ma = socketaddr_to_multiaddr(&local_addr, version);
            pending_event = Some(TransportEvent::NewAddress {
                listener_id,
                listen_addr: ma,
            })
        }

        let endpoint_c = endpoint.clone();
        let accept = async move { endpoint_c.accept().await }.boxed();

        Ok(Listener {
            endpoint,
            socket,
            accept,
            listener_id,
            version,
            handshake_timeout,
            if_watcher,
            is_closed: false,
            pending_event,
            close_listener_waker: None,
            listening_addresses,
        })
    }

    /// Report the listener as closed in a [`TransportEvent::ListenerClosed`] and
    /// terminate the stream.
    fn close(&mut self, reason: Result<(), Error>) {
        if self.is_closed {
            return;
        }
        self.endpoint.close(From::from(0u32), &[]);
        self.pending_event = Some(TransportEvent::ListenerClosed {
            listener_id: self.listener_id,
            reason,
        });
        self.is_closed = true;

        // Wake the stream to deliver the last event.
        if let Some(waker) = self.close_listener_waker.take() {
            waker.wake();
        }
    }

    /// Clone underlying socket (for hole punching).
    fn try_clone_socket(&self) -> std::io::Result<UdpSocket> {
        self.socket.try_clone()
    }

    fn socket_addr(&self) -> SocketAddr {
        self.socket
            .local_addr()
            .expect("Cannot fail because the socket is bound")
    }

    /// Poll for a next If Event.
    fn poll_if_addr(&mut self, cx: &mut Context<'_>) -> Poll<<Self as Stream>::Item> {
        let endpoint_addr = self.socket_addr();
        let Some(if_watcher) = self.if_watcher.as_mut() else {
            return Poll::Pending;
        };
        loop {
            match ready!(P::poll_if_event(if_watcher, cx)) {
                Ok(IfEvent::Up(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listenaddr(&endpoint_addr, inet.addr(), self.version)
                    {
                        tracing::debug!(
                            address=%listen_addr,
                            "New listen address"
                        );
                        self.listening_addresses.insert(inet.addr());
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listenaddr(&endpoint_addr, inet.addr(), self.version)
                    {
                        tracing::debug!(
                            address=%listen_addr,
                            "Expired listen address"
                        );
                        self.listening_addresses.remove(&inet.addr());
                        return Poll::Ready(TransportEvent::AddressExpired {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Err(err) => {
                    return Poll::Ready(TransportEvent::ListenerError {
                        listener_id: self.listener_id,
                        error: err.into(),
                    })
                }
            }
        }
    }
}

impl<P: Provider> Stream for Listener<P> {
    type Item = TransportEvent<<GenTransport<P> as Transport>::ListenerUpgrade, Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.pending_event.take() {
                return Poll::Ready(Some(event));
            }
            if self.is_closed {
                return Poll::Ready(None);
            }
            if let Poll::Ready(event) = self.poll_if_addr(cx) {
                return Poll::Ready(Some(event));
            }

            match self.accept.poll_unpin(cx) {
                Poll::Ready(Some(incoming)) => {
                    let endpoint = self.endpoint.clone();
                    self.accept = async move { endpoint.accept().await }.boxed();

                    let connecting = match incoming.accept() {
                        Ok(connecting) => connecting,
                        Err(error) => {
                            return Poll::Ready(Some(TransportEvent::ListenerError {
                                listener_id: self.listener_id,
                                error: Error::Connection(crate::ConnectionError(error)),
                            }))
                        }
                    };

                    let local_addr = socketaddr_to_multiaddr(&self.socket_addr(), self.version);
                    let remote_addr = connecting.remote_address();
                    let send_back_addr = socketaddr_to_multiaddr(&remote_addr, self.version);

                    let event = TransportEvent::Incoming {
                        upgrade: Connecting::new(connecting, self.handshake_timeout),
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    };
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(None) => {
                    self.close(Ok(()));
                    continue;
                }
                Poll::Pending => {}
            };

            self.close_listener_waker = Some(cx.waker().clone());

            return Poll::Pending;
        }
    }
}

impl<P: Provider> fmt::Debug for Listener<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener")
            .field("listener_id", &self.listener_id)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("is_closed", &self.is_closed)
            .field("pending_event", &self.pending_event)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtocolVersion {
    V1, // i.e. RFC9000
    Draft29,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SocketFamily {
    Ipv4,
    Ipv6,
}

impl SocketFamily {
    fn is_same(a: &IpAddr, b: &IpAddr) -> bool {
        matches!(
            (a, b),
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_))
        )
    }
}

impl From<IpAddr> for SocketFamily {
    fn from(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => SocketFamily::Ipv4,
            IpAddr::V6(_) => SocketFamily::Ipv6,
        }
    }
}

/// Turn an [`IpAddr`] reported by the interface watcher into a
/// listen-address for the endpoint.
///
/// For this, the `ip` is combined with the port that the endpoint
/// is actually bound.
///
/// Returns `None` if the `ip` is not the same socket family as the
/// address that the endpoint is bound to.
fn ip_to_listenaddr(
    endpoint_addr: &SocketAddr,
    ip: IpAddr,
    version: ProtocolVersion,
) -> Option<Multiaddr> {
    // True if either both addresses are Ipv4 or both Ipv6.
    if !SocketFamily::is_same(&endpoint_addr.ip(), &ip) {
        return None;
    }
    let socket_addr = SocketAddr::new(ip, endpoint_addr.port());
    Some(socketaddr_to_multiaddr(&socket_addr, version))
}

/// Tries to turn a QUIC multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
fn multiaddr_to_socketaddr(
    addr: &Multiaddr,
    support_draft_29: bool,
) -> Option<(SocketAddr, ProtocolVersion, Option<PeerId>)> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    let mut peer_id = None;
    for proto in iter {
        match proto {
            Protocol::P2p(id) => {
                peer_id = Some(id);
            }
            _ => return None,
        }
    }
    let version = match proto3 {
        Protocol::QuicV1 => ProtocolVersion::V1,
        Protocol::Quic if support_draft_29 => ProtocolVersion::Draft29,
        _ => return None,
    };

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Udp(port)) => {
            Some((SocketAddr::new(ip.into(), port), version, peer_id))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port)) => {
            Some((SocketAddr::new(ip.into(), port), version, peer_id))
        }
        _ => None,
    }
}

/// Turns an IP address and port into the corresponding QUIC multiaddr.
fn socketaddr_to_multiaddr(socket_addr: &SocketAddr, version: ProtocolVersion) -> Multiaddr {
    let quic_proto = match version {
        ProtocolVersion::V1 => Protocol::QuicV1,
        ProtocolVersion::Draft29 => Protocol::Quic,
    };
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(quic_proto)
}

#[cfg(test)]
#[cfg(any(feature = "async-std", feature = "tokio"))]
mod tests {
    use futures::future::poll_fn;

    use super::*;

    #[test]
    fn multiaddr_to_udp_conversion() {
        assert!(multiaddr_to_socketaddr(
            &"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap(),
            true
        )
        .is_none());

        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/12345/quic-v1"
                    .parse::<Multiaddr>()
                    .unwrap(),
                false
            ),
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345,),
                ProtocolVersion::V1,
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/255.255.255.255/udp/8080/quic-v1"
                    .parse::<Multiaddr>()
                    .unwrap(),
                false
            ),
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 8080,),
                ProtocolVersion::V1,
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/55148/quic-v1/p2p/12D3KooW9xk7Zp1gejwfwNpfm6L9zH5NL4Bx5rm94LRYJJHJuARZ"
                    .parse::<Multiaddr>()
                    .unwrap(), false
            ),
            Some((SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                55148,
            ), ProtocolVersion::V1, Some("12D3KooW9xk7Zp1gejwfwNpfm6L9zH5NL4Bx5rm94LRYJJHJuARZ".parse().unwrap())))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/::1/udp/12345/quic-v1".parse::<Multiaddr>().unwrap(),
                false
            ),
            Some((
                SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345,),
                ProtocolVersion::V1,
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/udp/8080/quic-v1"
                    .parse::<Multiaddr>()
                    .unwrap(),
                false
            ),
            Some((
                SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::new(
                        65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
                    )),
                    8080,
                ),
                ProtocolVersion::V1,
                None
            ))
        );

        assert!(multiaddr_to_socketaddr(
            &"/ip4/127.0.0.1/udp/1234/quic".parse::<Multiaddr>().unwrap(),
            false
        )
        .is_none());
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/1234/quic".parse::<Multiaddr>().unwrap(),
                true
            ),
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 1234,),
                ProtocolVersion::Draft29,
                None
            ))
        );
    }

    #[cfg(feature = "async-std")]
    #[async_std::test]
    async fn test_close_listener() {
        let keypair = libp2p_identity::Keypair::generate_ed25519();
        let config = Config::new(&keypair);
        let mut transport = crate::async_std::Transport::new(config);
        assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
            .now_or_never()
            .is_none());

        // Run test twice to check that there is no unexpected behaviour if `Transport.listener`
        // is temporarily empty.
        for _ in 0..2 {
            let id = ListenerId::next();
            transport
                .listen_on(id, "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
                .unwrap();

            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::NewAddress {
                    listener_id,
                    listen_addr,
                } => {
                    assert_eq!(listener_id, id);
                    assert!(
                        matches!(listen_addr.iter().next(), Some(Protocol::Ip4(a)) if !a.is_unspecified())
                    );
                    assert!(
                        matches!(listen_addr.iter().nth(1), Some(Protocol::Udp(port)) if port != 0)
                    );
                    assert!(matches!(listen_addr.iter().nth(2), Some(Protocol::QuicV1)));
                }
                e => panic!("Unexpected event: {e:?}"),
            }
            assert!(transport.remove_listener(id), "Expect listener to exist.");
            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::ListenerClosed {
                    listener_id,
                    reason: Ok(()),
                } => {
                    assert_eq!(listener_id, id);
                }
                e => panic!("Unexpected event: {e:?}"),
            }
            // Poll once again so that the listener has the chance to return `Poll::Ready(None)` and
            // be removed from the list of listeners.
            assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
                .now_or_never()
                .is_none());
            assert!(transport.listeners.is_empty());
        }
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_dialer_drop() {
        let keypair = libp2p_identity::Keypair::generate_ed25519();
        let config = Config::new(&keypair);
        let mut transport = crate::tokio::Transport::new(config);

        let _dial = transport
            .dial(
                "/ip4/123.45.67.8/udp/1234/quic-v1".parse().unwrap(),
                DialOpts {
                    role: Endpoint::Dialer,
                    port_use: PortUse::Reuse,
                },
            )
            .unwrap();

        assert!(transport.dialer.contains_key(&SocketFamily::Ipv4));
        assert!(!transport.dialer.contains_key(&SocketFamily::Ipv6));

        // Start listening so that the dialer and driver are dropped.
        transport
            .listen_on(
                ListenerId::next(),
                "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
            )
            .unwrap();
        assert!(!transport.dialer.contains_key(&SocketFamily::Ipv4));
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_listens_ipv4_ipv6_separately() {
        let keypair = libp2p_identity::Keypair::generate_ed25519();
        let config = Config::new(&keypair);
        let mut transport = crate::tokio::Transport::new(config);
        let port = {
            let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
            socket.local_addr().unwrap().port()
        };

        transport
            .listen_on(
                ListenerId::next(),
                format!("/ip4/0.0.0.0/udp/{port}/quic-v1").parse().unwrap(),
            )
            .unwrap();
        transport
            .listen_on(
                ListenerId::next(),
                format!("/ip6/::/udp/{port}/quic-v1").parse().unwrap(),
            )
            .unwrap();
    }
}
