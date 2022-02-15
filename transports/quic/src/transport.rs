// Copyright 2021 David Craven.
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

use crate::endpoint::{EndpointConfig, TransportChannel};
use crate::muxer::QuicMuxer;
use crate::{QuicConfig, QuicError};
use futures::channel::oneshot;
use futures::prelude::*;
use if_watch::{IfEvent, IfWatcher};
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerBox};
use libp2p_core::transport::{Boxed, ListenerEvent, Transport, TransportError};
use libp2p_core::PeerId;
use parking_lot::Mutex;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use udp_socket::SocketType;

#[derive(Clone)]
pub struct QuicTransport {
    inner: Arc<Mutex<QuicTransportInner>>,
}

impl QuicTransport {
    /// Creates a new quic transport.
    pub async fn new(
        config: QuicConfig,
        addr: Multiaddr,
    ) -> Result<Self, TransportError<QuicError>> {
        let socket_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        let addresses = if socket_addr.ip().is_unspecified() {
            let watcher = IfWatcher::new()
                .await
                .map_err(|err| TransportError::Other(err.into()))?;
            Addresses::Unspecified(watcher)
        } else {
            Addresses::Ip(Some(socket_addr.ip()))
        };
        let endpoint = EndpointConfig::new(config, socket_addr).map_err(TransportError::Other)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(QuicTransportInner {
                channel: endpoint.spawn(),
                addresses,
            })),
        })
    }

    /// Creates a boxed libp2p transport.
    pub fn boxed(self) -> Boxed<(PeerId, StreamMuxerBox)> {
        Transport::map(self, |(peer_id, muxer), _| {
            (peer_id, StreamMuxerBox::new(muxer))
        })
        .boxed()
    }
}

impl std::fmt::Debug for QuicTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("QuicTransport").finish()
    }
}

struct QuicTransportInner {
    channel: TransportChannel,
    addresses: Addresses,
}

enum Addresses {
    Unspecified(IfWatcher),
    Ip(Option<IpAddr>),
}

impl Transport for QuicTransport {
    type Output = (PeerId, QuicMuxer);
    type Error = QuicError;
    type Listener = Self;
    type ListenerUpgrade = QuicUpgrade;
    type Dial = QuicDial;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;
        Ok(self)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = multiaddr_to_socketaddr(&addr).ok_or_else(|| {
            tracing::debug!("invalid multiaddr");
            TransportError::MultiaddrNotSupported(addr.clone())
        })?;
        if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            tracing::debug!("invalid multiaddr");
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        tracing::debug!("dialing {}", socket_addr);
        let rx = self.inner.lock().channel.dial(socket_addr);
        Ok(QuicDial::Dialing(rx))
    }

    fn dial_as_listener(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // TODO: As the listener of a QUIC hole punch, we need to send a random UDP packet to the
        // `addr`. See DCUtR specification below.
        //
        // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
        self.dial(addr)
    }

    fn address_translation(&self, _listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        Some(observed.clone())
    }
}

impl Stream for QuicTransport {
    type Item = Result<ListenerEvent<QuicUpgrade, QuicError>, QuicError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut inner = self.inner.lock();
        match &mut inner.addresses {
            Addresses::Ip(ip) => {
                if let Some(ip) = ip.take() {
                    let addr = socketaddr_to_multiaddr(&SocketAddr::new(ip, inner.channel.port()));
                    return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr))));
                }
            }
            Addresses::Unspecified(watcher) => match Pin::new(watcher).poll(cx) {
                Poll::Ready(Ok(IfEvent::Up(net))) => {
                    if inner.channel.ty() == SocketType::Ipv4 && net.addr().is_ipv4()
                        || inner.channel.ty() != SocketType::Ipv4 && net.addr().is_ipv6()
                    {
                        let addr = socketaddr_to_multiaddr(&SocketAddr::new(
                            net.addr(),
                            inner.channel.port(),
                        ));
                        return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr))));
                    }
                }
                Poll::Ready(Ok(IfEvent::Down(net))) => {
                    if inner.channel.ty() == SocketType::Ipv4 && net.addr().is_ipv4()
                        || inner.channel.ty() != SocketType::Ipv4 && net.addr().is_ipv6()
                    {
                        let addr = socketaddr_to_multiaddr(&SocketAddr::new(
                            net.addr(),
                            inner.channel.port(),
                        ));
                        return Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(addr))));
                    }
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err.into()))),
                Poll::Pending => {}
            },
        }
        match inner.channel.poll_incoming(cx) {
            Poll::Ready(Some(Ok(muxer))) => Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                local_addr: muxer.local_addr(),
                remote_addr: muxer.remote_addr(),
                upgrade: QuicUpgrade::new(muxer),
            }))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum QuicDial {
    Dialing(oneshot::Receiver<Result<QuicMuxer, QuicError>>),
    Upgrade(QuicUpgrade),
}

impl Future for QuicDial {
    type Output = Result<(PeerId, QuicMuxer), QuicError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match &mut *self {
                Self::Dialing(rx) => match Pin::new(rx).poll(cx) {
                    Poll::Ready(Ok(Ok(muxer))) => {
                        *self = Self::Upgrade(QuicUpgrade::new(muxer));
                    }
                    Poll::Ready(Ok(Err(err))) => return Poll::Ready(Err(err)),
                    Poll::Ready(Err(_)) => panic!("endpoint crashed"),
                    Poll::Pending => return Poll::Pending,
                },
                Self::Upgrade(upgrade) => return Pin::new(upgrade).poll(cx),
            }
        }
    }
}

pub struct QuicUpgrade {
    muxer: Option<QuicMuxer>,
}

impl QuicUpgrade {
    fn new(muxer: QuicMuxer) -> Self {
        Self { muxer: Some(muxer) }
    }
}

impl Future for QuicUpgrade {
    type Output = Result<(PeerId, QuicMuxer), QuicError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = Pin::into_inner(self);
        let muxer = inner.muxer.as_mut().expect("future polled after ready");
        match muxer.poll_event(cx) {
            Poll::Pending => {
                if let Some(peer_id) = muxer.peer_id() {
                    muxer.set_accept_incoming(true);
                    Poll::Ready(Ok((
                        peer_id,
                        inner.muxer.take().expect("future polled after ready"),
                    )))
                } else {
                    Poll::Pending
                }
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err.into())),
            Poll::Ready(Ok(_)) => {
                unreachable!("muxer.incoming is set to false so no events can be produced");
            }
        }
    }
}

/// Tries to turn a QUIC multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
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
mod tests {
    use super::*;

    #[test]
    fn multiaddr_to_socketaddr_conversion() {
        use std::net::{Ipv4Addr, Ipv6Addr};

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

        assert!(multiaddr_to_socketaddr(
            &"/ip4/127.0.0.1/udp/12345/quic/tcp/12345"
                .parse::<Multiaddr>()
                .unwrap()
        )
        .is_none());

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
}
