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
use std::{net::SocketAddr, pin::Pin, sync::Arc};
use std::task::{Poll, Context};

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
pub struct QuicTransport(pub Arc<Endpoint>, /* addr reported */ bool); // FIXME IfWatcher

impl QuicTransport {
    pub fn new(endpoint: Arc<Endpoint>) -> Self {
        Self(endpoint, false)
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
        Ok(self)
        // // TODO: check address correctness

        // // TODO: report the locally opened addresses

        // Ok(stream::unfold((), move |()| {
        //     let endpoint = self.0.clone();
        //     let addr = addr.clone();
        //     async move {
        //         let connec = endpoint.next_incoming().await;
        //         let remote_addr = socketaddr_to_multiaddr(&connec.remote_addr());
        //         let event = Ok(ListenerEvent::Upgrade {
        //             upgrade: Upgrade::from_connection(connec),
        //             local_addr: addr.clone(), // TODO: hack
        //             remote_addr,
        //         });
        //         Some((event, ()))
        //     }
        // })
        // .boxed())
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
            let connection = self.0.dial(socket_addr).await.map_err(Error::Reach)?;
            let final_connec = Upgrade::from_connection(connection).await?;
            Ok(final_connec)
        }
        .boxed())
    }
}


impl Stream for QuicTransport {
    type Item = Result<ListenerEvent<Upgrade, Error>, Error>;

    #[tracing::instrument(skip_all)]
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        tracing::trace!("QuicTransport::poll_next");
        let endpoint = self.0.clone();

        if !self.1 { // FIXME IfWatcher
            self.1 = true;
            let addr = socketaddr_to_multiaddr(&endpoint.local_addr);
            return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr))));
        }


        //let addr = addr.clone();

            let connec = match endpoint.poll_incoming(cx) {
                Poll::Ready(Some(conn)) => conn,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            };
            let remote_addr = socketaddr_to_multiaddr(&connec.remote_addr());
            let event = ListenerEvent::Upgrade {
                upgrade: Upgrade::from_connection(connec),
                local_addr: "/ip4/127.0.0.1/udp/0/quic".parse().unwrap(), // addr.clone(), // TODO: hack
                remote_addr,
            };
            Poll::Ready(Some(Ok(event)))
            //Some((event, ()))
    
        //Poll::Pending
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
