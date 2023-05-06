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

use crate::endpoint::{Config, QuinnConfig, ToEndpoint};
use crate::provider::Provider;
use crate::{endpoint, Connecting, Connection, Error};

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::ready;
use futures::stream::StreamExt;
use futures::{prelude::*, stream::SelectAll};

use if_watch::IfEvent;

use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
    Transport,
};
use libp2p_identity::PeerId;
use std::collections::hash_map::{DefaultHasher, Entry};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::time::Duration;
use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll, Waker},
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
    /// Config for the inner [`quinn_proto`] structs.
    quinn_config: QuinnConfig,
    /// Timeout for the [`Connecting`] future.
    handshake_timeout: Duration,
    /// Whether draft-29 is supported for dialing and listening.
    support_draft_29: bool,
    /// Streams of active [`Listener`]s.
    listeners: SelectAll<Listener<P>>,
    /// Dialer for each socket family if no matching listener exists.
    dialer: HashMap<SocketFamily, Dialer>,
    /// Waker to poll the transport again when a new dialer or listener is added.
    waker: Option<Waker>,
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
        }
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
        let (socket_addr, version) = multiaddr_to_socketaddr(&addr, self.support_draft_29)
            .ok_or(TransportError::MultiaddrNotSupported(addr))?;
        let listener = Listener::new(
            listener_id,
            socket_addr,
            self.quinn_config.clone(),
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

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        if !is_quic_addr(listen, self.support_draft_29)
            || !is_quic_addr(observed, self.support_draft_29)
        {
            return None;
        }
        Some(observed.clone())
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (socket_addr, version) = multiaddr_to_socketaddr(&addr, self.support_draft_29)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let mut listeners = self
            .listeners
            .iter_mut()
            .filter(|l| {
                if l.is_closed {
                    return false;
                }
                let listen_addr = l.endpoint_channel.socket_addr();
                SocketFamily::is_same(&listen_addr.ip(), &socket_addr.ip())
                    && listen_addr.ip().is_loopback() == socket_addr.ip().is_loopback()
            })
            .collect::<Vec<_>>();

        let dialer_state = match listeners.len() {
            0 => {
                // No listener. Get or create an explicit dialer.
                let socket_family = socket_addr.ip().into();
                let dialer = match self.dialer.entry(socket_family) {
                    Entry::Occupied(occupied) => occupied.into_mut(),
                    Entry::Vacant(vacant) => {
                        if let Some(waker) = self.waker.take() {
                            waker.wake();
                        }
                        vacant.insert(Dialer::new::<P>(self.quinn_config.clone(), socket_family)?)
                    }
                };
                &mut dialer.state
            }
            1 => &mut listeners[0].dialer_state,
            _ => {
                // Pick any listener to use for dialing.
                // We hash the socket address to achieve determinism.
                let mut hasher = DefaultHasher::new();
                socket_addr.hash(&mut hasher);
                let index = hasher.finish() as usize % listeners.len();
                &mut listeners[index].dialer_state
            }
        };
        Ok(dialer_state.new_dial(socket_addr, self.handshake_timeout, version))
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        // TODO: As the listener of a QUIC hole punch, we need to send a random UDP packet to the
        // `addr`. See DCUtR specification below.
        //
        // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let mut errored = Vec::new();
        for (key, dialer) in &mut self.dialer {
            if let Poll::Ready(_error) = dialer.poll(cx) {
                errored.push(*key);
            }
        }

        for key in errored {
            // Endpoint driver of dialer crashed.
            // Drop dialer and all pending dials so that the connection receiver is notified.
            self.dialer.remove(&key);
        }

        if let Poll::Ready(Some(ev)) = self.listeners.poll_next_unpin(cx) {
            return Poll::Ready(ev);
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

/// Dialer for addresses if no matching listener exists.
#[derive(Debug)]
struct Dialer {
    /// Channel to the [`crate::endpoint::Driver`] that
    /// is driving the endpoint.
    endpoint_channel: endpoint::Channel,
    /// Queued dials for the endpoint.
    state: DialerState,
}

impl Dialer {
    fn new<P: Provider>(
        config: QuinnConfig,
        socket_family: SocketFamily,
    ) -> Result<Self, TransportError<Error>> {
        let endpoint_channel = endpoint::Channel::new_dialer::<P>(config, socket_family)
            .map_err(TransportError::Other)?;
        Ok(Dialer {
            endpoint_channel,
            state: DialerState::default(),
        })
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Error> {
        self.state.poll(&mut self.endpoint_channel, cx)
    }
}

impl Drop for Dialer {
    fn drop(&mut self) {
        self.endpoint_channel.send_on_drop(ToEndpoint::Decoupled);
    }
}

/// Pending dials to be sent to the endpoint was the [`endpoint::Channel`]
/// has capacity
#[derive(Default, Debug)]
struct DialerState {
    pending_dials: VecDeque<ToEndpoint>,
    waker: Option<Waker>,
}

impl DialerState {
    fn new_dial(
        &mut self,
        address: SocketAddr,
        timeout: Duration,
        version: ProtocolVersion,
    ) -> BoxFuture<'static, Result<(PeerId, Connection), Error>> {
        let (rx, tx) = oneshot::channel();

        let message = ToEndpoint::Dial {
            addr: address,
            result: rx,
            version,
        };

        self.pending_dials.push_back(message);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        async move {
            // Our oneshot getting dropped means the message didn't make it to the endpoint driver.
            let connection = tx.await.map_err(|_| Error::EndpointDriverCrashed)??;
            let (peer, connection) = Connecting::new(connection, timeout).await?;

            Ok((peer, connection))
        }
        .boxed()
    }

    /// Send all pending dials into the given [`endpoint::Channel`].
    ///
    /// This only ever returns [`Poll::Pending`], or an error in case the channel is closed.
    fn poll(&mut self, channel: &mut endpoint::Channel, cx: &mut Context<'_>) -> Poll<Error> {
        while let Some(to_endpoint) = self.pending_dials.pop_front() {
            match channel.try_send(to_endpoint, cx) {
                Ok(Ok(())) => {}
                Ok(Err(to_endpoint)) => {
                    self.pending_dials.push_front(to_endpoint);
                    break;
                }
                Err(endpoint::Disconnected {}) => return Poll::Ready(Error::EndpointDriverCrashed),
            }
        }
        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// Listener for incoming connections.
struct Listener<P: Provider> {
    /// Id of the listener.
    listener_id: ListenerId,

    version: ProtocolVersion,

    /// Channel to the endpoint to initiate dials.
    endpoint_channel: endpoint::Channel,
    /// Queued dials.
    dialer_state: DialerState,

    /// Channel where new connections are being sent.
    new_connections_rx: mpsc::Receiver<Connection>,
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
}

impl<P: Provider> Listener<P> {
    fn new(
        listener_id: ListenerId,
        socket_addr: SocketAddr,
        config: QuinnConfig,
        handshake_timeout: Duration,
        version: ProtocolVersion,
    ) -> Result<Self, Error> {
        let (endpoint_channel, new_connections_rx) =
            endpoint::Channel::new_bidirectional::<P>(config, socket_addr)?;

        let if_watcher;
        let pending_event;
        if socket_addr.ip().is_unspecified() {
            if_watcher = Some(P::new_if_watcher()?);
            pending_event = None;
        } else {
            if_watcher = None;
            let ma = socketaddr_to_multiaddr(endpoint_channel.socket_addr(), version);
            pending_event = Some(TransportEvent::NewAddress {
                listener_id,
                listen_addr: ma,
            })
        }

        Ok(Listener {
            endpoint_channel,
            listener_id,
            version,
            new_connections_rx,
            handshake_timeout,
            if_watcher,
            is_closed: false,
            pending_event,
            dialer_state: DialerState::default(),
            close_listener_waker: None,
        })
    }

    /// Report the listener as closed in a [`TransportEvent::ListenerClosed`] and
    /// terminate the stream.
    fn close(&mut self, reason: Result<(), Error>) {
        if self.is_closed {
            return;
        }
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

    /// Poll for a next If Event.
    fn poll_if_addr(&mut self, cx: &mut Context<'_>) -> Poll<<Self as Stream>::Item> {
        let if_watcher = match self.if_watcher.as_mut() {
            Some(iw) => iw,
            None => return Poll::Pending,
        };
        loop {
            match ready!(P::poll_if_event(if_watcher, cx)) {
                Ok(IfEvent::Up(inet)) => {
                    if let Some(listen_addr) = ip_to_listenaddr(
                        self.endpoint_channel.socket_addr(),
                        inet.addr(),
                        self.version,
                    ) {
                        log::debug!("New listen address: {}", listen_addr);
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if let Some(listen_addr) = ip_to_listenaddr(
                        self.endpoint_channel.socket_addr(),
                        inet.addr(),
                        self.version,
                    ) {
                        log::debug!("Expired listen address: {}", listen_addr);
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

    /// Poll [`DialerState`] to initiate requested dials.
    fn poll_dialer(&mut self, cx: &mut Context<'_>) -> Poll<Error> {
        let Self {
            dialer_state,
            endpoint_channel,
            ..
        } = self;

        dialer_state.poll(endpoint_channel, cx)
    }
}

impl<P: Provider> Stream for Listener<P> {
    type Item = TransportEvent<Connecting, Error>;
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
            if let Poll::Ready(error) = self.poll_dialer(cx) {
                self.close(Err(error));
                continue;
            }
            match self.new_connections_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(connection)) => {
                    let local_addr = socketaddr_to_multiaddr(connection.local_addr(), self.version);
                    let send_back_addr =
                        socketaddr_to_multiaddr(&connection.remote_addr(), self.version);
                    let event = TransportEvent::Incoming {
                        upgrade: Connecting::new(connection, self.handshake_timeout),
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    };
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(None) => {
                    self.close(Err(Error::EndpointDriverCrashed));
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
            .field("endpoint_channel", &self.endpoint_channel)
            .field("dialer_state", &self.dialer_state)
            .field("new_connections_rx", &self.new_connections_rx)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("is_closed", &self.is_closed)
            .field("pending_event", &self.pending_event)
            .finish()
    }
}

impl<P: Provider> Drop for Listener<P> {
    fn drop(&mut self) {
        self.endpoint_channel.send_on_drop(ToEndpoint::Decoupled);
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
) -> Option<(SocketAddr, ProtocolVersion)> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    for proto in iter {
        match proto {
            Protocol::P2p(_) => {} // Ignore a `/p2p/...` prefix of possibly outer protocols, if present.
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
            Some((SocketAddr::new(ip.into(), port), version))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port)) => {
            Some((SocketAddr::new(ip.into(), port), version))
        }
        _ => None,
    }
}

/// Whether an [`Multiaddr`] is a valid for the QUIC transport.
fn is_quic_addr(addr: &Multiaddr, support_draft_29: bool) -> bool {
    use Protocol::*;
    let mut iter = addr.iter();
    let first = match iter.next() {
        Some(p) => p,
        None => return false,
    };
    let second = match iter.next() {
        Some(p) => p,
        None => return false,
    };
    let third = match iter.next() {
        Some(p) => p,
        None => return false,
    };
    let fourth = iter.next();
    let fifth = iter.next();

    matches!(first, Ip4(_) | Ip6(_) | Dns(_) | Dns4(_) | Dns6(_))
        && matches!(second, Udp(_))
        && if support_draft_29 {
            matches!(third, QuicV1 | Quic)
        } else {
            matches!(third, QuicV1)
        }
        && matches!(fourth, Some(P2p(_)) | None)
        && matches!(fifth, None)
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
mod test {
    use futures::future::poll_fn;
    use futures_timer::Delay;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
                ProtocolVersion::V1
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
                ProtocolVersion::V1
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
            ), ProtocolVersion::V1))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/::1/udp/12345/quic-v1".parse::<Multiaddr>().unwrap(),
                false
            ),
            Some((
                SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345,),
                ProtocolVersion::V1
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
                ProtocolVersion::V1
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
                ProtocolVersion::Draft29
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

            // Copy channel to use it later.
            let mut channel = transport
                .listeners
                .iter()
                .next()
                .unwrap()
                .endpoint_channel
                .clone();

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

            // Check that the [`Driver`] has shut down.
            Delay::new(Duration::from_millis(10)).await;
            poll_fn(|cx| {
                assert!(channel.try_send(ToEndpoint::Decoupled, cx).is_err());
                Poll::Ready(())
            })
            .await;
        }
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn test_dialer_drop() {
        let keypair = libp2p_identity::Keypair::generate_ed25519();
        let config = Config::new(&keypair);
        let mut transport = crate::tokio::Transport::new(config);

        let _dial = transport
            .dial("/ip4/123.45.67.8/udp/1234/quic-v1".parse().unwrap())
            .unwrap();

        // Expect a dialer and its background task to exist.
        let mut channel = transport
            .dialer
            .get(&SocketFamily::Ipv4)
            .unwrap()
            .endpoint_channel
            .clone();
        assert!(!transport.dialer.contains_key(&SocketFamily::Ipv6));

        // Send dummy dial to check that the endpoint driver is running.
        poll_fn(|cx| {
            let (tx, _) = oneshot::channel();
            let _ = channel
                .try_send(
                    ToEndpoint::Dial {
                        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
                        result: tx,
                        version: ProtocolVersion::V1,
                    },
                    cx,
                )
                .unwrap();
            Poll::Ready(())
        })
        .await;

        // Start listening so that the dialer and driver are dropped.
        transport
            .listen_on(
                ListenerId::next(),
                "/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap(),
            )
            .unwrap();
        assert!(!transport.dialer.contains_key(&SocketFamily::Ipv4));

        // Check that the [`Driver`] has shut down.
        Delay::new(Duration::from_millis(10)).await;
        poll_fn(|cx| {
            assert!(channel.try_send(ToEndpoint::Decoupled, cx).is_err());
            Poll::Ready(())
        })
        .await;
    }
}
