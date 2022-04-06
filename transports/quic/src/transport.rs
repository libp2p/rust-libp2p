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

use crate::{endpoint::Endpoint, in_addr::InAddr, muxer::QuicMuxer, upgrade::Upgrade};

use futures::prelude::*;
use futures::stream::StreamExt;

use if_watch::IfEvent;

use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    PeerId, Transport,
};
use std::task::{Context, Poll};
use std::{net::SocketAddr, pin::Pin, sync::Arc};

// We reexport the errors that are exposed in the API.
// All of these types use one another.
pub use crate::connection::Error as Libp2pQuicConnectionError;
pub use quinn_proto::{
    ApplicationClose, ConfigError, ConnectError, ConnectionClose, ConnectionError,
    TransportError as QuinnTransportError, TransportErrorCode,
};

/// Wraps around an `Arc<Endpoint>` and implements the [`Transport`] trait.
///
/// > **Note**: This type is necessary because Rust unfortunately forbids implementing the
/// >           `Transport` trait directly on `Arc<Endpoint>`.
#[derive(Debug, Clone)]
pub struct QuicTransport {
    endpoint: Arc<Endpoint>,
    /// The IP addresses of network interfaces on which the listening socket
    /// is accepting connections.
    ///
    /// If the listen socket listens on all interfaces, these may change over
    /// time as interfaces become available or unavailable.
    in_addr: InAddr,
}

impl QuicTransport {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        let in_addr = InAddr::new(endpoint.local_addr.ip());
        Self { endpoint, in_addr }
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
    /// Error while working with IfWatcher.
    #[error("{0}")]
    IfWatcher(std::io::Error),
}

impl Transport for QuicTransport {
    type Output = (PeerId, QuicMuxer);
    type Error = Error;
    // type Listener = Pin<
    //     Box<dyn Stream<Item = Result<ListenerEvent<Upgrade, Self::Error>, Self::Error>> + Send>,
    // >;
    type Listener = Self;
    type ListenerUpgrade = Upgrade;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    #[tracing::instrument]
    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;
        Ok(self)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        Some(observed.clone())
    }

    #[tracing::instrument]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = if let Some(socket_addr) = multiaddr_to_socketaddr(&addr) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                tracing::error!("multiaddr not supported");
                return Err(TransportError::MultiaddrNotSupported(addr));
            }
            socket_addr
        } else {
            tracing::error!("multiaddr not supported");
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        Ok(async move {
            let connection = self
                .endpoint
                .dial(socket_addr)
                .await
                .map_err(Error::Reach)?;
            let final_connec = Upgrade::from_connection(connection).await?;
            Ok(final_connec)
        }
        .boxed())
    }

    fn dial_as_listener(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // TODO: As the listener of a QUIC hole punch, we need to send a random UDP packet to the
        // `addr`. See DCUtR specification below.
        //
        // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
        self.dial(addr)
    }
}

impl Stream for QuicTransport {
    type Item = Result<ListenerEvent<Upgrade, Error>, Error>;

    #[tracing::instrument(skip_all)]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);
        let endpoint = me.endpoint.as_ref();

        // Poll for a next IfEvent
        match me.in_addr.poll_next_unpin(cx) {
            Poll::Ready(mut item) => {
                if let Some(item) = item.take() {
                    // Consume all events for up/down interface changes.
                    match item {
                        Ok(IfEvent::Up(inet)) => {
                            let ip = inet.addr();
                            if endpoint.local_addr.is_ipv4() == ip.is_ipv4() {
                                let socket_addr = SocketAddr::new(ip, endpoint.local_addr.port());
                                let ma = socketaddr_to_multiaddr(&socket_addr);
                                tracing::debug!("New listen address: {}", ma);
                                return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(ma))));
                            }
                        }
                        Ok(IfEvent::Down(inet)) => {
                            let ip = inet.addr();
                            if endpoint.local_addr.is_ipv4() == ip.is_ipv4() {
                                let socket_addr = SocketAddr::new(ip, endpoint.local_addr.port());
                                let ma = socketaddr_to_multiaddr(&socket_addr);
                                tracing::debug!("Expired listen address: {}", ma);
                                return Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(ma))));
                            }
                        }
                        Err(err) => {
                            tracing::debug! {
                                "Failure polling interfaces: {:?}.",
                                err
                            };
                            return Poll::Ready(Some(Ok(ListenerEvent::Error(Error::IfWatcher(
                                err,
                            )))));
                        }
                    }
                }
            }
            Poll::Pending => {
                // continue polling endpoint
            }
        }

        let connection = match endpoint.poll_incoming(cx) {
            Poll::Ready(Some(conn)) => conn,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };
        let local_addr = socketaddr_to_multiaddr(&connection.local_addr());
        let remote_addr = socketaddr_to_multiaddr(&connection.remote_addr());
        let event = ListenerEvent::Upgrade {
            upgrade: Upgrade::from_connection(connection),
            local_addr,
            remote_addr,
        };
        Poll::Ready(Some(Ok(event)))
    }
}

/// Tries to turn a QUIC multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
pub(crate) fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    while let Some(proto) = iter.next() {
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
#[test]
fn multiaddr_to_udp_conversion() {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    assert!(
        multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap()).is_none()
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
