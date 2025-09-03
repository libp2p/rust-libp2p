use std::net::{IpAddr, SocketAddr};

use libp2p_core::{multiaddr::Protocol, Multiaddr};

use crate::fingerprint::Fingerprint;

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

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn parse_valid_address_with_certhash_and_p2p() {
        let addr = "/ip4/127.0.0.1/udp/39901/webrtc-direct/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w/p2p/12D3KooWNpDk9w6WrEEcdsEH1y47W71S36yFjw4sd3j7omzgCSMS"
            .parse()
            .unwrap();

        let maybe_parsed = parse_webrtc_dial_addr(&addr);

        assert_eq!(
            maybe_parsed,
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 39901),
                Fingerprint::raw(hex_literal::hex!(
                    "e2929e4a5548242ed6b512350df8829b1e4f9d50183c5732a07f99d7c4b2b8eb"
                ))
            ))
        );
    }

    #[test]
    fn peer_id_is_not_required() {
        let addr = "/ip4/127.0.0.1/udp/39901/webrtc-direct/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w"
            .parse()
            .unwrap();

        let maybe_parsed = parse_webrtc_dial_addr(&addr);

        assert_eq!(
            maybe_parsed,
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 39901),
                Fingerprint::raw(hex_literal::hex!(
                    "e2929e4a5548242ed6b512350df8829b1e4f9d50183c5732a07f99d7c4b2b8eb"
                ))
            ))
        );
    }

    #[test]
    fn parse_ipv6() {
        let addr =
            "/ip6/::1/udp/12345/webrtc-direct/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w/p2p/12D3KooWNpDk9w6WrEEcdsEH1y47W71S36yFjw4sd3j7omzgCSMS"
                .parse()
                .unwrap();

        let maybe_parsed = parse_webrtc_dial_addr(&addr);

        assert_eq!(
            maybe_parsed,
            Some((
                SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 12345),
                Fingerprint::raw(hex_literal::hex!(
                    "e2929e4a5548242ed6b512350df8829b1e4f9d50183c5732a07f99d7c4b2b8eb"
                ))
            ))
        );
    }
}
