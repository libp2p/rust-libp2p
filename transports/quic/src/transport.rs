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
use crate::Config;
use crate::{endpoint::Endpoint, muxer::QuicMuxer, upgrade::Upgrade};

use futures::channel::{mpsc, oneshot};
use futures::ready;
use futures::stream::StreamExt;
use futures::{prelude::*, stream::SelectAll};

use if_watch::{IfEvent, IfWatcher};

use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
    PeerId, Transport,
};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::task::Waker;
use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

// We reexport the errors that are exposed in the API.
// All of these types use one another.
pub use crate::connection::Error as Libp2pQuicConnectionError;
pub use quinn_proto::{
    ApplicationClose, ConfigError, ConnectError, ConnectionClose, ConnectionError,
    TransportError as QuinnTransportError, TransportErrorCode,
};

#[derive(Debug)]
pub struct QuicTransport {
    config: Config,
    listeners: SelectAll<Listener>,
    /// Dialer for Ipv4 addresses if no matching listener exists.
    ipv4_dialer: Option<Dialer>,
    /// Dialer for Ipv6 addresses if no matching listener exists.
    ipv6_dialer: Option<Dialer>,
}

impl QuicTransport {
    pub fn new(config: Config) -> Self {
        Self {
            listeners: SelectAll::new(),
            config,
            ipv4_dialer: None,
            ipv6_dialer: None,
        }
    }
}

/// Error that can happen on the transport.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error while trying to reach a remote.
    #[error("{0}")]
    Reach(ConnectError),
    /// Error after the remote has been reached.
    #[error("{0}")]
    Established(Libp2pQuicConnectionError),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("Background task crashed.")]
    TaskCrashed,
}

impl Transport for QuicTransport {
    type Output = (PeerId, QuicMuxer);
    type Error = Error;
    type ListenerUpgrade = Upgrade;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let socket_addr =
            multiaddr_to_socketaddr(&addr).ok_or(TransportError::MultiaddrNotSupported(addr))?;
        let listener_id = ListenerId::new();
        let listener = Listener::new(listener_id, socket_addr, self.config.clone())
            .map_err(TransportError::Other)?;
        self.listeners.push(listener);
        // Drop reference to dialer endpoint so that the endpoint is dropped once the last
        // connection that uses it is closed.
        // New outbound connections will use a bidirectional (listener) endpoint.
        match socket_addr {
            SocketAddr::V4(_) => self.ipv4_dialer.take(),
            SocketAddr::V6(_) => self.ipv6_dialer.take(),
        };
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

    fn address_translation(&self, _server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        Some(observed.clone())
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        let mut listeners = self
            .listeners
            .iter_mut()
            .filter(|l| {
                let listen_addr = l.endpoint.socket_addr();
                listen_addr.is_ipv4() == socket_addr.is_ipv4()
                    && listen_addr.ip().is_loopback() == socket_addr.ip().is_loopback()
            })
            .collect::<Vec<_>>();

        let (tx, rx) = oneshot::channel();
        let to_endpoint = ToEndpoint::Dial {
            addr: socket_addr,
            result: tx,
        };
        if listeners.is_empty() {
            let dialer = match socket_addr {
                SocketAddr::V4(_) => &mut self.ipv4_dialer,
                SocketAddr::V6(_) => &mut self.ipv6_dialer,
            };
            if dialer.is_none() {
                let _ = dialer.insert(Dialer::new(self.config.clone(), socket_addr.is_ipv6())?);
            }
            dialer
                .as_mut()
                .unwrap()
                .pending_dials
                .push_back(to_endpoint);
        } else {
            // Pick a random listener to use for dialing.
            let n = rand::random::<usize>() % listeners.len();
            let listener = listeners.get_mut(n).expect("Can not be out of bound.");
            listener.pending_dials.push_back(to_endpoint);
            if let Some(waker) = listener.waker.take() {
                waker.wake()
            }
        };

        Ok(async move {
            let connection = rx
                .await
                .map_err(|_| Error::TaskCrashed)?
                .map_err(Error::Reach)?;
            let final_connec = Upgrade::from_connection(connection).await?;
            Ok(final_connec)
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
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
        if let Some(dialer) = self.ipv4_dialer.as_mut() {
            if dialer.drive_dials(cx).is_err() {
                // Background task of dialer crashed.
                // Drop dialer and all pending dials so that the connection receiver is notified.
                self.ipv4_dialer = None;
            }
        }
        if let Some(dialer) = self.ipv6_dialer.as_mut() {
            if dialer.drive_dials(cx).is_err() {
                // Background task of dialer crashed.
                // Drop dialer and all pending dials so that the connection receiver is notified.
                self.ipv4_dialer = None;
            }
        }
        match self.listeners.poll_next_unpin(cx) {
            Poll::Ready(Some(ev)) => Poll::Ready(ev),
            _ => Poll::Pending,
        }
    }
}

#[derive(Debug)]
struct Dialer {
    endpoint: Endpoint,
    pending_dials: VecDeque<ToEndpoint>,
}

impl Dialer {
    fn new(config: Config, is_ipv6: bool) -> Result<Self, TransportError<Error>> {
        let endpoint = Endpoint::new_dialer(config, is_ipv6).map_err(TransportError::Other)?;
        Ok(Dialer {
            endpoint,
            pending_dials: VecDeque::new(),
        })
    }

    fn drive_dials(&mut self, cx: &mut Context<'_>) -> Result<(), mpsc::SendError> {
        if let Some(to_endpoint) = self.pending_dials.pop_front() {
            match self.endpoint.try_send(to_endpoint, cx) {
                Ok(Ok(())) => {}
                Ok(Err(to_endpoint)) => self.pending_dials.push_front(to_endpoint),
                Err(err) => {
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Listener {
    endpoint: Endpoint,

    listener_id: ListenerId,

    /// Channel where new connections are being sent.
    new_connections_rx: mpsc::Receiver<Connection>,

    if_watcher: Option<IfWatcher>,

    /// Whether the listener was closed and the stream should terminate.
    is_closed: bool,

    /// Pending event to reported.
    pending_event: Option<<Self as Stream>::Item>,

    pending_dials: VecDeque<ToEndpoint>,

    waker: Option<Waker>,
}

impl Listener {
    fn new(
        listener_id: ListenerId,
        socket_addr: SocketAddr,
        config: Config,
    ) -> Result<Self, Error> {
        let (endpoint, new_connections_rx) = Endpoint::new_bidirectional(config, socket_addr)?;

        let if_watcher;
        let pending_event;
        if socket_addr.ip().is_unspecified() {
            if_watcher = Some(IfWatcher::new()?);
            pending_event = None;
        } else {
            if_watcher = None;
            let ma = socketaddr_to_multiaddr(endpoint.socket_addr());
            pending_event = Some(TransportEvent::NewAddress {
                listener_id,
                listen_addr: ma,
            })
        }

        Ok(Listener {
            endpoint,
            listener_id,
            new_connections_rx,
            if_watcher,
            is_closed: false,
            pending_event,
            pending_dials: VecDeque::new(),
            waker: None,
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
                    if let Some(listen_addr) = ip_to_listenaddr(&self.endpoint, inet.addr()) {
                        tracing::debug!("New listen address: {}", listen_addr);
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if let Some(listen_addr) = ip_to_listenaddr(&self.endpoint, inet.addr()) {
                        tracing::debug!("Expired listen address: {}", listen_addr);
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

impl Stream for Listener {
    type Item = TransportEvent<<QuicTransport as Transport>::ListenerUpgrade, Error>;
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
            if let Some(to_endpoint) = self.pending_dials.pop_front() {
                match self.endpoint.try_send(to_endpoint, cx) {
                    Ok(Ok(())) => {}
                    Ok(Err(to_endpoint)) => self.pending_dials.push_front(to_endpoint),
                    Err(_) => {
                        self.close(Err(Error::TaskCrashed));
                        continue;
                    }
                }
            }
            match self.new_connections_rx.poll_next_unpin(cx) {
                Poll::Ready(Some(connection)) => {
                    let local_addr = connection
                        .local_addr()
                        .expect("exists for server connections.");
                    let local_addr = socketaddr_to_multiaddr(&local_addr);
                    let send_back_addr = socketaddr_to_multiaddr(&connection.remote_addr());
                    let event = TransportEvent::Incoming {
                        upgrade: Upgrade::from_connection(connection),
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    };
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(None) => {
                    self.close(Err(Error::TaskCrashed));
                    continue;
                }
                Poll::Pending => {}
            };
            self.waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
    }
}

/// Turn an [`IpAddr`] into a listen-address for the endpoint.
///
/// Returns `None` if the address is not the same socket family as the
/// address that the endpoint is bound to.
pub fn ip_to_listenaddr(endpoint: &Endpoint, ip: IpAddr) -> Option<Multiaddr> {
    // True if either both addresses are Ipv4 or both Ipv6.
    let is_same_ip_family = endpoint.socket_addr().is_ipv4() == ip.is_ipv4();
    if !is_same_ip_family {
        return None;
    }
    let socket_addr = SocketAddr::new(ip, endpoint.socket_addr().port());
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

/// Turns an IP address and port into the corresponding QUIC multiaddr.
pub(crate) fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::Quic)
}

#[cfg(test)]
mod test {

    use futures::future::poll_fn;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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

    #[async_std::test]
    async fn close_listener() {
        let keypair = libp2p_core::identity::Keypair::generate_ed25519();
        let mut transport = QuicTransport::new(Config::new(&keypair).unwrap());

        assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
            .now_or_never()
            .is_none());

        // Run test twice to check that there is no unexpected behaviour if `QuicTransport.listener`
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
