use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use libp2p_identity::PeerId;
use multibase::Base;

const CID_V1: u8 = 0x01;
const LIBP2P_KEY_CODEC: u8 = 0x72;

/// Encode a [`PeerId`] as the base36 CIDv1 (`libp2p-key`) label used in `libp2p.direct` hostnames.
pub fn peer_id_label(peer_id: PeerId) -> String {
    let multihash = peer_id.to_bytes();
    let mut cid = Vec::with_capacity(2 + multihash.len());
    cid.push(CID_V1);
    cid.push(LIBP2P_KEY_CODEC);
    cid.extend_from_slice(&multihash);
    multibase::encode(Base::Base36Lower, cid)
}

/// Encode an IP address as the dashed DNS label used in `libp2p.direct` hostnames.
pub fn ip_label(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(ip) => ipv4_label(ip),
        IpAddr::V6(ip) => ipv6_label(ip),
    }
}

/// Encode an IPv4 address by replacing `.` with `-`, e.g. `1.2.3.4` becomes `1-2-3-4`.
pub fn ipv4_label(ip: Ipv4Addr) -> String {
    ip.to_string().replace('.', "-")
}

/// Encode an IPv6 address by replacing `:` with `-`, padding a leading or trailing `-` with `0` so
/// the result is a valid DNS label, e.g. `::1` becomes `0--1` and `2001:db8::1` becomes
/// `2001-db8--1`.
///
/// The address is rendered in its canonical [RFC 5952](https://datatracker.ietf.org/doc/html/rfc5952) compressed form before substitution, which
/// matches the labels produced by the `p2p-forge` DNS server.
pub fn ipv6_label(ip: Ipv6Addr) -> String {
    let mut label = ip.to_string().replace(':', "-");
    if label.starts_with('-') {
        label.insert(0, '0');
    }
    if label.ends_with('-') {
        label.push('0');
    }
    label
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv6Addr, str::FromStr};

    use super::*;

    #[test]
    fn peer_id_label_interop() {
        let vectors = [
            (
                "12D3KooWGzxzKZYveHXtpG6AsrUJBcWxHBFS2HsEoGTxrMLvKXtf",
                "k51qzi5uqu5diuci8bva7narzo109juvlfbckhzf3j2ljua2979b21rs6uyquk",
            ),
            (
                "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                "k2k4r8jl0yz8qjgqbmc2cdu5hkqek5rj6flgnlkyywynci20j0iuyfuj",
            ),
        ];
        for (peer_id, expected) in vectors {
            let peer_id = PeerId::from_str(peer_id).unwrap();
            assert_eq!(peer_id_label(peer_id), expected);
        }
    }

    #[test]
    fn ipv4_label_interop() {
        assert_eq!(ipv4_label(Ipv4Addr::new(1, 2, 3, 4)), "1-2-3-4");
        assert_eq!(
            ipv4_label(Ipv4Addr::new(255, 255, 255, 255)),
            "255-255-255-255"
        );
    }

    #[test]
    fn ipv6_label_interop() {
        let vectors = [
            ("::1", "0--1"),
            ("::", "0--0"),
            ("1::", "1--0"),
            ("fe80::", "fe80--0"),
            ("2001:db8::1", "2001-db8--1"),
            ("2001:4860:4860::8889", "2001-4860-4860--8889"),
            ("a:b:c:d:1:2:3:4", "a-b-c-d-1-2-3-4"),
        ];
        for (ip, expected) in vectors {
            let ip = Ipv6Addr::from_str(ip).unwrap();
            assert_eq!(ipv6_label(ip), expected, "ipv6 label for {ip}");
        }
    }
}
