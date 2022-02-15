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

use crate::{endpoint::Endpoint, muxer::QuicMuxer, upgrade::Upgrade};

use futures::prelude::*;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    PeerId, Transport,
};
use std::{net::{SocketAddr, IpAddr}, pin::Pin, sync::Arc};
use std::task::{Poll, Context};
use if_watch::{IfEvent, IfWatcher};
use futures::future::BoxFuture;
use futures::lock::Mutex;

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
        let in_addr = if endpoint.local_addr.ip().is_unspecified() {
            let watcher = IfWatch::Pending(IfWatcher::new().boxed());
            InAddr::Any { if_watch: Arc::new(Mutex::new(watcher)) }
        } else {
            InAddr::One { ip: Some(endpoint.local_addr.ip()) }
        };
        Self { endpoint, in_addr }
    }
}

enum IfWatch {
    Pending(BoxFuture<'static, std::io::Result<IfWatcher>>),
    Ready(IfWatcher),
}

impl std::fmt::Debug for IfWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            IfWatch::Pending(_) => write!(f, "Pending"),
            IfWatch::Ready(_) => write!(f, "Ready"),
        }
    }
}

/// The listening addresses of a `UdpSocket`.
#[derive(Clone, Debug)]
enum InAddr {
    /// The socket accepts connections on a single interface.
    One {
        ip: Option<IpAddr>,
    },
    /// The socket accepts connections on all interfaces.
    Any {
        if_watch: Arc<Mutex<IfWatch>>,
    },
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
      panic!("not implemented")
    }

    #[tracing::instrument]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = if let Some(socket_addr) = multiaddr_to_socketaddr(&addr) {
            // FIXME IfWatcher
            // if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            //     tracing::error!("multiaddr not supported");
            //     return Err(TransportError::MultiaddrNotSupported(addr));
            // }
            socket_addr
        } else {
            tracing::error!("multiaddr not supported");
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        Ok(async move {
            let connection = self.endpoint.dial(socket_addr).await.map_err(Error::Reach)?;
            let final_connec = Upgrade::from_connection(connection).await?;
            Ok(final_connec)
        }
        .boxed())
    }
}


impl Stream for QuicTransport {
    type Item = Result<ListenerEvent<Upgrade, Error>, Error>;

    #[tracing::instrument(skip_all)]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);
        let endpoint = me.endpoint.as_ref();

        loop {
            match &mut me.in_addr {
                // If the listener is bound to a single interface, make sure the
                // address is reported once.
                InAddr::One { ip } => {
                    if let Some(ip) = ip.take() {
                        let socket_addr = SocketAddr::new(ip, endpoint.local_addr.port());
                        let ma = socketaddr_to_multiaddr(&socket_addr);
                        return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(ma))));
                    }
                },
                InAddr::Any { if_watch } => {
                    let mut ifwatch_lock = if_watch.lock();
                    let mut ifwatch = futures::ready!(ifwatch_lock.poll_unpin(cx)); // TODO continue
                    match &mut *ifwatch {
                        // If we listen on all interfaces, wait for `if-watch` to be ready.
                        IfWatch::Pending(f) => {
                            // let mut f_lock = f.lock();
                            // let mut f = futures::ready!(f_lock.poll_unpin(cx));
                            match futures::ready!(f.poll_unpin(cx)) {
                                Ok(w) => {
                                    *ifwatch = IfWatch::Ready(w);
                                    continue;
                                }
                                Err(err) => {
                                    tracing::debug! {
                                        "Failed to begin observing interfaces: {:?}. Scheduling retry.",
                                        err
                                    };
                                    *ifwatch = IfWatch::Pending(IfWatcher::new().boxed());
                                    //me.pause = Some(Delay::new(me.sleep_on_error));
                                    return Poll::Ready(Some(Ok(ListenerEvent::Error(Error::IfWatcher(err)))));
                                }
                            }
                        },
                        // Consume all events for up/down interface changes.
                        IfWatch::Ready(watch) => {
                            while let Poll::Ready(ev) = watch.poll_unpin(cx) {
                                match ev {
                                    Ok(IfEvent::Up(inet)) => {
                                        let ip = inet.addr();
                                        if endpoint.local_addr.is_ipv4() == ip.is_ipv4()
                                        {
                                            let socket_addr = SocketAddr::new(ip, endpoint.local_addr.port());
                                            let ma = socketaddr_to_multiaddr(&socket_addr);
                                            tracing::debug!("New listen address: {}", ma);
                                            return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(
                                                ma,
                                            ))));
                                        }
                                    }
                                    Ok(IfEvent::Down(inet)) => {
                                        let ip = inet.addr();
                                        if endpoint.local_addr.is_ipv4() == ip.is_ipv4()
                                        {
                                            let socket_addr = SocketAddr::new(ip, endpoint.local_addr.port());
                                            let ma = socketaddr_to_multiaddr(&socket_addr);
                                            tracing::debug!("Expired listen address: {}", ma);
                                            return Poll::Ready(Some(Ok(
                                                ListenerEvent::AddressExpired(ma),
                                            )));
                                        }
                                    }
                                    Err(err) => {
                                        tracing::debug! {
                                            "Failure polling interfaces: {:?}. Scheduling retry.",
                                            err
                                        };
                                        return Poll::Ready(Some(Ok(ListenerEvent::Error(Error::IfWatcher(err)))));
                                    }
                                }
                            }
                        }
                    }
                },
            }
            break;
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
