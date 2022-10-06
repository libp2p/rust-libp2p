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

//! Implementation of the [`Transport`] trait for QUIC.
//!
//! Combines all the objects in the other modules to implement the trait.

use crate::connection::Connection;
use crate::endpoint::ToEndpoint;
use crate::provider::Provider;
use crate::{endpoint, muxer::QuicMuxer, upgrade::Connecting};
use crate::{Config, ConnectionError};

use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, MapErr};
use futures::ready;
use futures::stream::StreamExt;
use futures::{prelude::*, stream::SelectAll};

use if_watch::{IfEvent, IfWatcher};

use futures::channel::oneshot::{Canceled, Receiver};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError as CoreTransportError, TransportEvent},
    PeerId, Transport,
};
use quinn_proto::ConnectError;
use rand::prelude::SliceRandom;
use rand::thread_rng;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use std::net::IpAddr;
use std::task::Waker;
use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug)]
pub struct GenTransport<P> {
    config: Config,
    listeners: SelectAll<Listener>,
    /// Dialer for each socket family if no matching listener exists.
    dialer: HashMap<SocketFamily, Dialer>,

    _marker: PhantomData<P>,
}

impl<P> GenTransport<P> {
    pub fn new(config: Config) -> Self {
        Self {
            listeners: SelectAll::new(),
            config,
            dialer: HashMap::new(),
            _marker: Default::default(),
        }
    }
}

/// Error that can happen on the transport.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Error while trying to reach a remote.
    #[error(transparent)]
    Reach(#[from] quinn_proto::ConnectError),
    /// Error after the remote has been reached.
    #[error(transparent)]
    Established(#[from] ConnectionError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The task driving the endpoint has crashed.
    #[error("Endpoint driver crashed")]
    EndpointDriverCrashed,
}

impl<P: Provider> Transport for GenTransport<P> {
    type Output = (PeerId, QuicMuxer);
    type Error = TransportError;
    type ListenerUpgrade = Connecting;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        addr: Multiaddr,
    ) -> Result<ListenerId, CoreTransportError<Self::Error>> {
        let socket_addr = multiaddr_to_socketaddr(&addr)
            .ok_or(CoreTransportError::MultiaddrNotSupported(addr))?;
        let listener_id = ListenerId::new();
        let listener = Listener::new::<P>(listener_id, socket_addr, self.config.clone())?;
        self.listeners.push(listener);

        // Remove dialer endpoint so that the endpoint is dropped once the last
        // connection that uses it is closed.
        // New outbound connections will use a bidirectional (listener) endpoint.
        self.dialer.remove(&socket_addr.ip().into());

        Ok(listener_id)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(listener) = self.listeners.iter_mut().find(|l| l.listener_id == id) {
            listener.close(Ok(()));
            true
        } else {
            false
        }
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        if !is_quic_addr(listen) || !is_quic_addr(observed) {
            return None;
        }
        Some(observed.clone())
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, CoreTransportError<Self::Error>> {
        let socket_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| CoreTransportError::MultiaddrNotSupported(addr.clone()))?;
        if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            return Err(CoreTransportError::MultiaddrNotSupported(addr));
        }
        let mut listeners = self
            .listeners
            .iter_mut()
            .filter(|l| {
                let listen_addr = l.endpoint_channel.socket_addr();
                SocketFamily::is_same(&listen_addr.ip(), &socket_addr.ip())
                    && listen_addr.ip().is_loopback() == socket_addr.ip().is_loopback()
            })
            .collect::<Vec<_>>();

        // Try to use pick a random listener to use for dialing.
        let dialing = match listeners.choose_mut(&mut thread_rng()) {
            Some(listener) => listener.dialer_state.new_dial(socket_addr),
            None => {
                // No listener? Get or create an explicit dialer.

                let socket_family = socket_addr.ip().into();
                let dialer = match self.dialer.entry(socket_family) {
                    Entry::Occupied(occupied) => occupied.into_mut(),
                    Entry::Vacant(vacant) => {
                        vacant.insert(Dialer::new::<P>(self.config.clone(), socket_family)?)
                    }
                };

                dialer.state.new_dial(socket_addr)
            }
        };

        Ok(async move {
            let connection = dialing.await??;
            let final_connec = Connecting::from_connection(connection).await?;
            Ok(final_connec)
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, CoreTransportError<Self::Error>> {
        // TODO: As the listener of a QUIC hole punch, we need to send a random UDP packet to the
        // `addr`. See DCUtR specification below.
        //
        // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
        self.dial(addr)
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
        match self.listeners.poll_next_unpin(cx) {
            Poll::Ready(Some(ev)) => Poll::Ready(ev),
            _ => Poll::Pending,
        }
    }
}

impl From<TransportError> for CoreTransportError<TransportError> {
    fn from(err: TransportError) -> Self {
        CoreTransportError::Other(err)
    }
}

#[derive(Debug)]
struct Dialer {
    endpoint_channel: endpoint::Channel,
    state: DialerState,
}

impl Dialer {
    fn new<P: Provider>(
        config: Config,
        socket_family: SocketFamily,
    ) -> Result<Self, CoreTransportError<TransportError>> {
        let endpoint_channel = endpoint::Channel::new_dialer::<P>(config, socket_family)
            .map_err(CoreTransportError::Other)?;
        Ok(Dialer {
            endpoint_channel,
            state: DialerState::default(),
        })
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<TransportError> {
        self.state.poll(&mut self.endpoint_channel, cx)
    }
}

impl Drop for Dialer {
    fn drop(&mut self) {
        self.endpoint_channel.send_on_drop(ToEndpoint::Decoupled);
    }
}

#[derive(Default, Debug)]
struct DialerState {
    pending_dials: VecDeque<ToEndpoint>,
    waker: Option<Waker>,
}

impl DialerState {
    // With TAIP, this return signature would be a bit nicer.
    fn new_dial(
        &mut self,
        address: SocketAddr,
    ) -> MapErr<Receiver<Result<Connection, ConnectError>>, fn(Canceled) -> TransportError> {
        let (rx, tx) = oneshot::channel();

        let message = ToEndpoint::Dial {
            addr: address,
            result: rx,
        };

        self.pending_dials.push_back(message);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        // Our oneshot getting dropped means the message didn't make it to the endpoint driver.
        tx.map_err(|_| TransportError::EndpointDriverCrashed)
    }

    /// Send all pending dials into the given [`endpoint::Channel`].
    ///
    /// This only ever returns [`Poll::Pending`] or an error in case the channel is closed.
    fn poll(
        &mut self,
        channel: &mut endpoint::Channel,
        cx: &mut Context<'_>,
    ) -> Poll<TransportError> {
        while let Some(to_endpoint) = self.pending_dials.pop_front() {
            match channel.try_send(to_endpoint, cx) {
                Ok(Ok(())) => {}
                Ok(Err(to_endpoint)) => {
                    self.pending_dials.push_front(to_endpoint);
                    break;
                }
                Err(endpoint::Disconnected {}) => {
                    return Poll::Ready(TransportError::EndpointDriverCrashed)
                }
            }
        }
        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[derive(Debug)]
struct Listener {
    endpoint_channel: endpoint::Channel,

    listener_id: ListenerId,

    /// Channel where new connections are being sent.
    new_connections_rx: mpsc::Receiver<Connection>,

    if_watcher: Option<IfWatcher>,

    /// Whether the listener was closed and the stream should terminate.
    is_closed: bool,

    /// Pending event to reported.
    pending_event: Option<<Self as Stream>::Item>,

    dialer_state: DialerState,

    waker: Option<Waker>,
}

impl Listener {
    fn new<P: Provider>(
        listener_id: ListenerId,
        socket_addr: SocketAddr,
        config: Config,
    ) -> Result<Self, TransportError> {
        let (endpoint_channel, new_connections_rx) =
            endpoint::Channel::new_bidirectional::<P>(config, socket_addr)?;

        let if_watcher;
        let pending_event;
        if socket_addr.ip().is_unspecified() {
            if_watcher = Some(IfWatcher::new()?);
            pending_event = None;
        } else {
            if_watcher = None;
            let ma = socketaddr_to_multiaddr(endpoint_channel.socket_addr());
            pending_event = Some(TransportEvent::NewAddress {
                listener_id,
                listen_addr: ma,
            })
        }

        Ok(Listener {
            endpoint_channel,
            listener_id,
            new_connections_rx,
            if_watcher,
            is_closed: false,
            pending_event,
            dialer_state: DialerState::default(),
            waker: None,
        })
    }

    /// Report the listener as closed in a [`TransportEvent::ListenerClosed`] and
    /// terminate the stream.
    fn close(&mut self, reason: Result<(), TransportError>) {
        if self.is_closed {
            return;
        }
        self.pending_event = Some(TransportEvent::ListenerClosed {
            listener_id: self.listener_id,
            reason,
        });
        self.is_closed = true;
    }

    /// Poll for a next If Event.
    fn poll_if_addr(&mut self, cx: &mut Context<'_>) -> Poll<<Self as Stream>::Item> {
        let if_watcher = match self.if_watcher.as_mut() {
            Some(iw) => iw,
            None => return Poll::Pending,
        };
        loop {
            match ready!(if_watcher.poll_if_event(cx)) {
                Ok(IfEvent::Up(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listenaddr(self.endpoint_channel.socket_addr(), inet.addr())
                    {
                        log::debug!("New listen address: {}", listen_addr);
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listenaddr(self.endpoint_channel.socket_addr(), inet.addr())
                    {
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
    fn poll_dialer(&mut self, cx: &mut Context<'_>) -> Poll<TransportError> {
        let Self {
            dialer_state,
            endpoint_channel,
            ..
        } = &mut *self;

        dialer_state.poll(endpoint_channel, cx)
    }
}

impl Stream for Listener {
    type Item = TransportEvent<Connecting, TransportError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.pending_event.take() {
                return Poll::Ready(Some(event));
            }
            if self.is_closed {
                return Poll::Ready(None);
            }
            match self.poll_if_addr(cx) {
                Poll::Ready(event) => return Poll::Ready(Some(event)),
                Poll::Pending => {}
            }
            match self.poll_dialer(cx) {
                Poll::Ready(error) => {
                    self.close(Err(error));
                    continue;
                }
                Poll::Pending => {}
            }
            match self.new_connections_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(connection)) => {
                    let local_addr = socketaddr_to_multiaddr(connection.local_addr());
                    let send_back_addr = socketaddr_to_multiaddr(&connection.remote_addr());
                    let event = TransportEvent::Incoming {
                        upgrade: Connecting::from_connection(connection),
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    };
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(None) => {
                    self.close(Err(TransportError::EndpointDriverCrashed));
                    continue;
                }
                Poll::Pending => {}
            };
            self.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.endpoint_channel.send_on_drop(ToEndpoint::Decoupled);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SocketFamily {
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

/// Turn an [`IpAddr`] into a listen-address for the endpoint.
///
/// Returns `None` if the address is not the same socket family as the
/// address that the endpoint is bound to.
fn ip_to_listenaddr(endpoint_addr: &SocketAddr, ip: IpAddr) -> Option<Multiaddr> {
    // True if either both addresses are Ipv4 or both Ipv6.
    if !SocketFamily::is_same(&endpoint_addr.ip(), &ip) {
        return None;
    }
    let socket_addr = SocketAddr::new(ip, endpoint_addr.port());
    Some(socketaddr_to_multiaddr(&socket_addr))
}

/// Tries to turn a QUIC multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
pub fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
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

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::Quic) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::Quic) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        _ => None,
    }
}

pub fn is_quic_addr(addr: &Multiaddr) -> bool {
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
    matches!(first, Ip4(_) | Ip6(_) | Dns(_) | Dns4(_) | Dns6(_))
        && matches!(second, Udp(_))
        && matches!(third, Quic)
}

/// Turns an IP address and port into the corresponding QUIC multiaddr.
pub(crate) fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::Quic)
}

#[cfg(test)]
#[cfg(any(feature = "async-std", feature = "tokio"))]
mod test {
    #[cfg(feature = "async-std")]
    use async_std_crate as async_std;
    use futures::future::poll_fn;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    #[cfg(feature = "tokio")]
    use tokio_crate as tokio;

    use super::*;

    #[test]
    fn multiaddr_to_udp_conversion() {
        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_none()
        );

        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/12345/quic"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some(SocketAddr::new(
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
            Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                8080,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/55148/quic/p2p/12D3KooW9xk7Zp1gejwfwNpfm6L9zH5NL4Bx5rm94LRYJJHJuARZ"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                55148,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip6/::1/udp/12345/quic".parse::<Multiaddr>().unwrap()),
            Some(SocketAddr::new(
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
            Some(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(
                    65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
                )),
                8080,
            ))
        );
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn tokio_close_listener() {
        let keypair = libp2p_core::identity::Keypair::generate_ed25519();
        let config = Config::new(&keypair).unwrap();
        let transport = crate::tokio::Transport::new(config.clone());
        test_close_listener(transport).await
    }

    #[cfg(feature = "async-std")]
    #[async_std::test]
    async fn async_std_close_listener() {
        let keypair = libp2p_core::identity::Keypair::generate_ed25519();
        let config = Config::new(&keypair).unwrap();
        let transport = crate::async_std::Transport::new(config.clone());
        test_close_listener(transport).await
    }

    async fn test_close_listener<P: Provider>(mut transport: GenTransport<P>) {
        assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
            .now_or_never()
            .is_none());

        // Run test twice to check that there is no unexpected behaviour if `GenTransport.listener`
        // is temporarily empty.
        for _ in 0..2 {
            let listener = transport
                .listen_on("/ip4/0.0.0.0/udp/0/quic".parse().unwrap())
                .unwrap();
            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::NewAddress {
                    listener_id,
                    listen_addr,
                } => {
                    assert_eq!(listener_id, listener);
                    assert!(
                        matches!(listen_addr.iter().next(), Some(Protocol::Ip4(a)) if !a.is_unspecified())
                    );
                    assert!(
                        matches!(listen_addr.iter().nth(1), Some(Protocol::Udp(port)) if port != 0)
                    );
                    assert!(matches!(listen_addr.iter().nth(2), Some(Protocol::Quic)));
                }
                e => panic!("Unexpected event: {:?}", e),
            }
            assert!(
                transport.remove_listener(listener),
                "Expect listener to exist."
            );
            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::ListenerClosed {
                    listener_id,
                    reason: Ok(()),
                } => {
                    assert_eq!(listener_id, listener);
                }
                e => panic!("Unexpected event: {:?}", e),
            }
            // Poll once again so that the listener has the chance to return `Poll::Ready(None)` and
            // be removed from the list of listeners.
            assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
                .now_or_never()
                .is_none());
            assert!(transport.listeners.is_empty());
        }
    }
}
