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
use crate::Config;
use crate::{endpoint::Endpoint, in_addr::InAddr, muxer::QuicMuxer, upgrade::Upgrade};

use futures::stream::StreamExt;
use futures::{channel::mpsc, prelude::*, stream::SelectAll};

use if_watch::IfEvent;

use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
    PeerId, Transport,
};
use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
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
    /// Endpoint to use if no listener exists.
    dialer: Option<Arc<Endpoint>>,
}

impl QuicTransport {
    pub fn new(config: Config) -> Self {
        Self {
            listeners: SelectAll::new(),
            config,
            dialer: None,
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
        let _ = self.dialer.take();
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
            tracing::error!("multiaddr not supported");
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        let endpoint = if self.listeners.is_empty() {
            match self.dialer.clone() {
                Some(endpoint) => endpoint,
                None => {
                    let endpoint =
                        Endpoint::new_dialer(self.config.clone()).map_err(TransportError::Other)?;
                    let _ = self.dialer.insert(endpoint.clone());
                    endpoint
                }
            }
        } else {
            // Pick a random listener to use for dialing.
            // TODO: Prefer listeners with same IP version.
            let n = rand::random::<usize>() % self.listeners.len();
            let listener = self
                .listeners
                .iter_mut()
                .nth(n)
                .expect("Can not be out of bound.");
            listener.endpoint.clone()
        };

        Ok(async move {
            let connection = endpoint.dial(socket_addr).await.map_err(Error::Reach)?;
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
        match self.listeners.poll_next_unpin(cx) {
            Poll::Ready(Some(ev)) => Poll::Ready(ev),
            _ => Poll::Pending,
        }
    }
}

#[derive(Debug)]
struct Listener {
    endpoint: Arc<Endpoint>,

    listener_id: ListenerId,

    /// Channel where new connections are being sent.
    new_connections_rx: mpsc::Receiver<Connection>,

    /// The IP addresses of network interfaces on which the listening socket
    /// is accepting connections.
    ///
    /// If the listen socket listens on all interfaces, these may change over
    /// time as interfaces become available or unavailable.
    in_addr: InAddr,

    /// Set to `Some` if this [`Listener`] should close.
    /// Optionally contains a [`TransportEvent::ListenerClosed`] that should be
    /// reported before the listener's stream is terminated.
    report_closed: Option<Option<<Self as Stream>::Item>>,
}

impl Listener {
    fn new(
        listener_id: ListenerId,
        socket_addr: SocketAddr,
        config: Config,
    ) -> Result<Self, Error> {
        let in_addr = InAddr::new(socket_addr.ip());
        let (endpoint, new_connections_rx) = Endpoint::new_bidirectional(config, socket_addr)?;
        Ok(Listener {
            endpoint,
            listener_id,
            new_connections_rx,
            in_addr,
            report_closed: None,
        })
    }

    /// Report the listener as closed in a [`TransportEvent::ListenerClosed`] and
    /// terminate the stream.
    fn close(&mut self, reason: Result<(), Error>) {
        match self.report_closed {
            Some(_) => tracing::debug!("Listener was already closed."),
            None => {
                // Report the listener event as closed.
                let _ = self
                    .report_closed
                    .insert(Some(TransportEvent::ListenerClosed {
                        listener_id: self.listener_id,
                        reason,
                    }));
            }
        }
    }

    /// Poll for a next If Event.
    fn poll_if_addr(&mut self, cx: &mut Context<'_>) -> Option<<Self as Stream>::Item> {
        loop {
            match self.in_addr.poll_next_unpin(cx) {
                Poll::Ready(mut item) => {
                    if let Some(item) = item.take() {
                        // Consume all events for up/down interface changes.
                        match item {
                            Ok(IfEvent::Up(inet)) => {
                                let ip = inet.addr();
                                if self.endpoint.socket_addr().is_ipv4() == ip.is_ipv4() {
                                    let socket_addr =
                                        SocketAddr::new(ip, self.endpoint.socket_addr().port());
                                    let ma = socketaddr_to_multiaddr(&socket_addr);
                                    tracing::debug!("New listen address: {}", ma);
                                    return Some(TransportEvent::NewAddress {
                                        listener_id: self.listener_id,
                                        listen_addr: ma,
                                    });
                                }
                            }
                            Ok(IfEvent::Down(inet)) => {
                                let ip = inet.addr();
                                if self.endpoint.socket_addr().is_ipv4() == ip.is_ipv4() {
                                    let socket_addr =
                                        SocketAddr::new(ip, self.endpoint.socket_addr().port());
                                    let ma = socketaddr_to_multiaddr(&socket_addr);
                                    tracing::debug!("Expired listen address: {}", ma);
                                    return Some(TransportEvent::AddressExpired {
                                        listener_id: self.listener_id,
                                        listen_addr: ma,
                                    });
                                }
                            }
                            Err(err) => {
                                tracing::debug! {
                                    "Failure polling interfaces: {:?}.",
                                    err
                                };
                                return Some(TransportEvent::ListenerError {
                                    listener_id: self.listener_id,
                                    error: err.into(),
                                });
                            }
                        }
                    }
                }
                Poll::Pending => return None,
            }
        }
    }
}

impl Stream for Listener {
    type Item = TransportEvent<<QuicTransport as Transport>::ListenerUpgrade, Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(closed) = self.report_closed.as_mut() {
            // Listener was closed.
            // Report the transport event if there is one. On the next iteration, return
            // `Poll::Ready(None)` to terminate the stream.
            return Poll::Ready(closed.take());
        }
        if let Some(event) = self.poll_if_addr(cx) {
            return Poll::Ready(Some(event));
        }
        let connection = match futures::ready!(self.new_connections_rx.poll_next_unpin(cx)) {
            Some(c) => c,
            None => {
                self.close(Err(Error::TaskCrashed));
                return self.poll_next(cx);
            }
        };

        let local_addr = socketaddr_to_multiaddr(&connection.local_addr());
        let send_back_addr = socketaddr_to_multiaddr(&connection.remote_addr());
        let event = TransportEvent::Incoming {
            upgrade: Upgrade::from_connection(connection),
            local_addr,
            send_back_addr,
            listener_id: self.listener_id,
        };
        Poll::Ready(Some(event))
    }
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
