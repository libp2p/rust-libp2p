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

use crate::crypto::Crypto;
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
use quinn_proto::crypto::Session;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use udp_socket::SocketType;

#[derive(Clone)]
pub struct QuicTransport<C: Crypto> {
    inner: Arc<Mutex<QuicTransportInner<C>>>,
}

impl<C: Crypto> QuicTransport<C>
where
    <C::Session as Session>::ClientConfig: Send + Unpin,
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    /// Creates a new quic transport.
    pub async fn new(
        config: QuicConfig<C>,
        addr: Multiaddr,
    ) -> Result<Self, TransportError<QuicError>> {
        let socket_addr = multiaddr_to_socketaddr::<C>(&addr)
            .map_err(|_| TransportError::MultiaddrNotSupported(addr.clone()))?
            .0;
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

impl<C: Crypto> std::fmt::Debug for QuicTransport<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("QuicTransport").finish()
    }
}

struct QuicTransportInner<C: Crypto> {
    channel: TransportChannel<C>,
    addresses: Addresses,
}

enum Addresses {
    Unspecified(IfWatcher),
    Ip(Option<IpAddr>),
}

impl<C: Crypto> Transport for QuicTransport<C>
where
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    type Output = (PeerId, QuicMuxer<C>);
    type Error = QuicError;
    type Listener = Self;
    type ListenerUpgrade = QuicUpgrade<C>;
    type Dial = QuicDial<C>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        multiaddr_to_socketaddr::<C>(&addr)
            .map_err(|_| TransportError::MultiaddrNotSupported(addr))?;
        Ok(self)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (socket_addr, public_key) =
            if let Ok((socket_addr, Some(public_key))) = multiaddr_to_socketaddr::<C>(&addr) {
                (socket_addr, public_key)
            } else {
                tracing::debug!("invalid multiaddr");
                return Err(TransportError::MultiaddrNotSupported(addr.clone()));
            };
        if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            tracing::debug!("invalid multiaddr");
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        tracing::debug!("dialing {}", socket_addr);
        let rx = self.inner.lock().channel.dial(socket_addr, public_key);
        Ok(QuicDial::Dialing(rx))
    }

    fn address_translation(&self, _listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        Some(observed.clone())
    }
}

impl<C: Crypto> Stream for QuicTransport<C> {
    type Item = Result<ListenerEvent<QuicUpgrade<C>, QuicError>, QuicError>;

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
pub enum QuicDial<C: Crypto> {
    Dialing(oneshot::Receiver<Result<QuicMuxer<C>, QuicError>>),
    Upgrade(QuicUpgrade<C>),
}

impl<C: Crypto> Future for QuicDial<C>
where
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    type Output = Result<(PeerId, QuicMuxer<C>), QuicError>;

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

pub struct QuicUpgrade<C: Crypto> {
    muxer: Option<QuicMuxer<C>>,
}

impl<C: Crypto> QuicUpgrade<C> {
    fn new(muxer: QuicMuxer<C>) -> Self {
        Self { muxer: Some(muxer) }
    }
}

impl<C: Crypto> Future for QuicUpgrade<C>
where
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    type Output = Result<(PeerId, QuicMuxer<C>), QuicError>;

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

/// Tries to turn a QUIC multiaddress into a UDP [`SocketAddr`]. Returns an error if the format
/// of the multiaddr is wrong.
fn multiaddr_to_socketaddr<C: Crypto>(
    addr: &Multiaddr,
) -> Result<(SocketAddr, Option<C::PublicKey>), ()> {
    let mut iter = addr.iter().peekable();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;
    let proto3 = iter.next().ok_or(())?;

    let peer_id = if let Some(Protocol::P2p(peer_id)) = iter.peek() {
        if peer_id.code() != multihash::Code::Identity.into() {
            return Err(());
        }
        let public_key =
            libp2p_core::PublicKey::from_protobuf_encoding(peer_id.digest()).map_err(|_| ())?;
        let public_key = C::extract_public_key(public_key).ok_or(())?;
        iter.next();
        Some(public_key)
    } else {
        None
    };

    if iter.next().is_some() {
        return Err(());
    }

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::Quic) => {
            Ok((SocketAddr::new(ip.into(), port), peer_id))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::Quic) => {
            Ok((SocketAddr::new(ip.into(), port), peer_id))
        }
        _ => Err(()),
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

    fn multiaddr_to_udp_conversion<C: Crypto>() {
        use std::net::{Ipv4Addr, Ipv6Addr};

        assert!(multiaddr_to_socketaddr::<C>(
            &"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap()
        )
        .is_err());

        assert_eq!(
            multiaddr_to_socketaddr::<C>(
                &"/ip4/127.0.0.1/udp/12345/quic"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345,),
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr::<C>(
                &"/ip4/255.255.255.255/udp/8080/quic"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 8080,),
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr::<C>(&"/ip6/::1/udp/12345/quic".parse::<Multiaddr>().unwrap()),
            Ok((
                SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345,),
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr::<C>(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/udp/8080/quic"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok((
                SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::new(
                        65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
                    )),
                    8080,
                ),
                None
            ))
        );
    }

    #[cfg(feature = "tls")]
    #[test]
    fn multiaddr_to_udp_tls() {
        multiaddr_to_udp_conversion::<crate::TlsCrypto>();
    }

    fn multiaddr_to_pk_conversion<C: Crypto>(keypair: C::Keypair) {
        use crate::crypto::ToLibp2p;
        use std::net::Ipv4Addr;

        let peer_id = keypair.to_public().to_peer_id();
        let addr = String::from("/ip4/127.0.0.1/udp/12345/quic/p2p/") + &peer_id.to_base58();
        assert_eq!(
            multiaddr_to_socketaddr::<C>(&addr.parse::<Multiaddr>().unwrap()),
            Ok((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345,),
                C::extract_public_key(keypair.to_public())
            ))
        );
    }

    #[cfg(feature = "tls")]
    #[test]
    fn multiaddr_to_pk_tls() {
        let keypair = libp2p_core::identity::Keypair::generate_ed25519();
        multiaddr_to_pk_conversion::<crate::TlsCrypto>(keypair);
    }
}
