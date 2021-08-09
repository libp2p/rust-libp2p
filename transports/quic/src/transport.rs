use crate::crypto::Crypto;
use crate::endpoint::{EndpointConfig, TransportChannel};
use crate::muxer::QuicMuxer;
use crate::{QuicConfig, QuicError};
use ed25519_dalek::PublicKey;
use futures::channel::oneshot;
use futures::prelude::*;
use if_watch::{IfEvent, IfWatcher};
use libp2p::core::muxing::{StreamMuxer, StreamMuxerBox};
use libp2p::core::transport::{Boxed, ListenerEvent, Transport, TransportError};
use libp2p::multiaddr::{Multiaddr, Protocol};
use libp2p::PeerId;
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
        let socket_addr = multiaddr_to_socketaddr(&addr)
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
        multiaddr_to_socketaddr(&addr).map_err(|_| TransportError::MultiaddrNotSupported(addr))?;
        Ok(self)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (socket_addr, public_key) =
            if let Ok((socket_addr, Some(public_key))) = multiaddr_to_socketaddr(&addr) {
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
                panic!("muxer.incoming is set to false so no events can be produced");
            }
        }
    }
}

/// Tries to turn a QUIC multiaddress into a UDP [`SocketAddr`]. Returns an error if the format
/// of the multiaddr is wrong.
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<(SocketAddr, Option<PublicKey>), ()> {
    let mut iter = addr.iter().peekable();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;
    let proto3 = iter.next().ok_or(())?;

    let peer_id = if let Some(Protocol::P2p(peer_id)) = iter.peek() {
        if peer_id.code() != multihash::Code::Identity.into() {
            return Err(());
        }
        let public_key =
            libp2p::core::PublicKey::from_protobuf_encoding(peer_id.digest()).map_err(|_| ())?;
        let public_key = if let libp2p::core::PublicKey::Ed25519(public_key) = public_key {
            public_key.encode()
        } else {
            return Err(());
        };
        let public_key = PublicKey::from_bytes(&public_key).map_err(|_| ())?;
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

    #[test]
    fn multiaddr_to_udp_conversion() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert_eq!(
            multiaddr_to_socketaddr(
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
            multiaddr_to_socketaddr(
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
            multiaddr_to_socketaddr(&"/ip6/::1/udp/12345/quic".parse::<Multiaddr>().unwrap()),
            Ok((
                SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345,),
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
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
}
