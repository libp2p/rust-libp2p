use crate::fingerprint::Fingerprint;
use libp2p_core::{multiaddr::Protocol, Multiaddr};
use std::net::{IpAddr, SocketAddr};

/// Parse the given [`Multiaddr`] into a [`SocketAddr`] and a [`Fingerprint`] for dialing.
pub fn parse_webrtc_dial_addr(addr: &Multiaddr) -> Option<(SocketAddr, Fingerprint)> {
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
