use futures::future::FutureExt;
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::{Boxed, ListenerId, Transport as _, TransportError, TransportEvent};
use libp2p_identity::{Keypair, PeerId};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::task::{Context, Poll};

// use crate::endpoint::Endpoint;
use super::fingerprint::Fingerprint;
use super::Connection;
use super::Error;

const HANDSHAKE_TIMEOUT_MS: u64 = 10_000;

/// Config for the [`Transport`].
#[derive(Clone)]
pub struct Config {
    keypair: Keypair,
}

/// A WebTransport [`Transport`](libp2p_core::Transport) that works with `web-sys`.
pub struct Transport {
    config: Config,
}

impl Config {
    /// Constructs a new configuration for the [`Transport`].
    pub fn new(keypair: &Keypair) -> Self {
        Config {
            keypair: keypair.to_owned(),
        }
    }
}

impl Transport {
    /// Constructs a new `Transport` with the given [`Config`].
    pub fn new(config: Config) -> Transport {
        Transport { config }
    }

    /// Wraps `Transport` in [`Boxed`] and makes it ready to be consumed by
    /// SwarmBuilder.
    pub fn boxed(self) -> Boxed<(PeerId, StreamMuxerBox)> {
        self.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed()
    }
}

impl libp2p_core::Transport for Transport {
    type Output = (PeerId, Connection);
    type Error = Error;
    type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(
        &mut self,
        _id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (sock_addr, server_fingerprint) = parse_webrtc_dial_addr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;

        if sock_addr.port() == 0 || sock_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let config = self.config.clone();
        let mut connection = Connection::new(sock_addr, server_fingerprint, config.keypair.clone());

        Ok(async move {
            let peer_id = connection.connect().await?;

            Ok((peer_id, connection))
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }

    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

/// Parse the given [`Multiaddr`] into a [`SocketAddr`] and a [`Fingerprint`] for dialing.
fn parse_webrtc_dial_addr(addr: &Multiaddr) -> Option<(SocketAddr, Fingerprint)> {
    let mut iter = addr.iter();

    let ip = match iter.next()? {
        Protocol::Ip4(ip) => IpAddr::from(ip),
        Protocol::Ip6(ip) => IpAddr::from(ip),
        _ => return None,
    };

    let port = iter.next()?;
    let webrtc = iter.next()?;
    let certhash = iter.next()?;

    let (port, fingerprint) = match (port, webrtc, certhash) {
        (Protocol::Udp(port), Protocol::WebRTCDirect, Protocol::Certhash(cert_hash)) => {
            let fingerprint = Fingerprint::try_from_multihash(cert_hash)?;

            (port, fingerprint)
        }
        _ => return None,
    };

    match iter.next() {
        Some(Protocol::P2p(_)) => {}
        // peer ID is optional
        None => {}
        // unexpected protocol
        Some(_) => return None,
    }

    Some((SocketAddr::new(ip, port), fingerprint))
}
