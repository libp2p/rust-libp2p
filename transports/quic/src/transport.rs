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
    transport::{ListenerId, TransportError, TransportEvent},
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

    listener: Option<(ListenerId, InAddr)>,
}

impl QuicTransport {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self {
            endpoint,
            listener: None
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
    /// Error while working with IfWatcher.
    #[error("{0}")]
    IfWatcher(std::io::Error),
}

impl Transport for QuicTransport {
    type Output = (PeerId, QuicMuxer);
    type Error = Error;
    type ListenerUpgrade = Upgrade;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;
        let listener = self.listener.get_or_insert((ListenerId::new(),  InAddr::new(self.endpoint.local_addr.ip())));
        Ok(listener.0)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some((listener_id, _)) = self.listener {
            if id == listener_id {
                self.listener = None;
                return true
            }
        }
        false
    }

    fn address_translation(&self, _server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        Some(observed.clone())
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
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

        let endpoint = self.endpoint.clone();

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
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let me = Pin::into_inner(self);
        // Poll for a next IfEvent
        let (listener_id, in_addr) = match me.listener.as_mut() {
            Some((id, in_addr)) => (*id, in_addr),
            None => return Poll::Pending
        };
        let endpoint = me.endpoint.as_ref();

        // Poll for a next IfEvent
        match in_addr.poll_next_unpin(cx) {
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
                                return Poll::Ready(TransportEvent::NewAddress {
                                    listener_id,
                                    listen_addr: ma,
                                });
                            }
                        }
                        Ok(IfEvent::Down(inet)) => {
                            let ip = inet.addr();
                            if endpoint.local_addr.is_ipv4() == ip.is_ipv4() {
                                let socket_addr = SocketAddr::new(ip, endpoint.local_addr.port());
                                let ma = socketaddr_to_multiaddr(&socket_addr);
                                tracing::debug!("Expired listen address: {}", ma);
                                return Poll::Ready(TransportEvent::AddressExpired {
                                    listener_id,
                                    listen_addr: ma,
                                });
                            }
                        }
                        Err(err) => {
                            tracing::debug! {
                                "Failure polling interfaces: {:?}.",
                                err
                            };
                            return Poll::Ready(TransportEvent::ListenerError {
                                listener_id,
                                error: Error::IfWatcher(err),
                            });
                        }
                    }
                }
            }
            Poll::Pending => {
                // continue polling endpoint
            }
        }

        let connection = match endpoint.poll_incoming(cx) {
            Poll::Ready(Some(connection)) => connection,
            Poll::Ready(None) => {
                return Poll::Ready(TransportEvent::ListenerClosed {
                    listener_id,
                    reason: Ok(()),
                })
            }
            Poll::Pending => return Poll::Pending,
        };
        let local_addr = socketaddr_to_multiaddr(&connection.local_addr());
        let send_back_addr = socketaddr_to_multiaddr(&connection.remote_addr());
        let event = TransportEvent::Incoming {
            upgrade: Upgrade::from_connection(connection),
            local_addr,
            send_back_addr,
            listener_id,
        };
        Poll::Ready(event)
    }
}

/// Tries to turn a QUIC multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
pub(crate) fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
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
