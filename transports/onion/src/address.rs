use arti_client::{DangerouslyIntoTorAddr, IntoTorAddr, TorAddr};
use libp2p_core::{multiaddr::Protocol, Multiaddr};
use std::net::SocketAddr;

/// "Dangerously" extract a Tor address from the provided [`Multiaddr`].
///
/// See [`DangerouslyIntoTorAddr`] for details around the safety / privacy considerations.
pub fn dangerous_extract_tor_address(multiaddr: &Multiaddr) -> Option<TorAddr> {
    if let Some(tor_addr) = safe_extract_tor_address(multiaddr) {
        return Some(tor_addr);
    }

    let mut protocols = multiaddr.into_iter();

    let tor_addr = try_to_socket_addr(&protocols.next()?, &protocols.next()?)?
        .into_tor_addr_dangerously()
        .ok()?;

    Some(tor_addr)
}

/// "Safely" extract a Tor address from the provided [`Multiaddr`].
///
/// See [`IntoTorAddr`] for details around the safety / privacy considerations.
pub fn safe_extract_tor_address(multiaddr: &Multiaddr) -> Option<TorAddr> {
    let mut protocols = multiaddr.into_iter();

    let tor_addr = try_to_domain_and_port(&protocols.next()?, &protocols.next()?)?
        .into_tor_addr()
        .ok()?;

    Some(tor_addr)
}

fn try_to_domain_and_port<'a, 'b>(
    maybe_domain: &'a Protocol,
    maybe_port: &'b Protocol,
) -> Option<(&'a str, u16)> {
    match (maybe_domain, maybe_port) {
        (
            Protocol::Dns(domain) | Protocol::Dns4(domain) | Protocol::Dns6(domain),
            Protocol::Tcp(port),
        ) => Some((domain.as_ref(), *port)),
        _ => None,
    }
}

fn try_to_socket_addr(maybe_ip: &Protocol, maybe_port: &Protocol) -> Option<SocketAddr> {
    match (maybe_ip, maybe_port) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Some(SocketAddr::from((*ip, *port))),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Some(SocketAddr::from((*ip, *port))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arti_client::TorAddr;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn extract_correct_address_from_dns() {
        let addresses = [
            "/dns/ip.tld/tcp/10".parse().unwrap(),
            "/dns4/dns.ip4.tld/tcp/11".parse().unwrap(),
            "/dns6/dns.ip6.tld/tcp/12".parse().unwrap(),
        ];

        let actual = addresses
            .iter()
            .filter_map(safe_extract_tor_address)
            .collect::<Vec<_>>();

        assert_eq!(
            &[
                TorAddr::from(("ip.tld", 10)).unwrap(),
                TorAddr::from(("dns.ip4.tld", 11)).unwrap(),
                TorAddr::from(("dns.ip6.tld", 12)).unwrap(),
            ],
            actual.as_slice()
        );
    }

    #[test]
    fn extract_correct_address_from_ips() {
        let addresses = [
            "/ip4/127.0.0.1/tcp/10".parse().unwrap(),
            "/ip6/::1/tcp/10".parse().unwrap(),
        ];

        let actual = addresses
            .iter()
            .filter_map(dangerous_extract_tor_address)
            .collect::<Vec<_>>();

        assert_eq!(
            &[
                TorAddr::dangerously_from((Ipv4Addr::LOCALHOST, 10)).unwrap(),
                TorAddr::dangerously_from((Ipv6Addr::LOCALHOST, 10)).unwrap(),
            ],
            actual.as_slice()
        );
    }

    #[test]
    fn dangerous_extract_works_on_domains_too() {
        let addresses = [
            "/dns/ip.tld/tcp/10".parse().unwrap(),
            "/ip4/127.0.0.1/tcp/10".parse().unwrap(),
            "/ip6/::1/tcp/10".parse().unwrap(),
        ];

        let actual = addresses
            .iter()
            .filter_map(dangerous_extract_tor_address)
            .collect::<Vec<_>>();

        assert_eq!(
            &[
                TorAddr::from(("ip.tld", 10)).unwrap(),
                TorAddr::dangerously_from((Ipv4Addr::LOCALHOST, 10)).unwrap(),
                TorAddr::dangerously_from((Ipv6Addr::LOCALHOST, 10)).unwrap(),
            ],
            actual.as_slice()
        );
    }

    #[test]
    fn detect_incorrect_address() {
        let addresses = [
            "/tcp/10/udp/12".parse().unwrap(),
            "/dns/ip.tld/dns4/ip.tld/dns6/ip.tld".parse().unwrap(),
            "/tcp/10/ip4/1.1.1.1".parse().unwrap(),
        ];

        let all_correct = addresses
            .iter()
            .map(safe_extract_tor_address)
            .all(|res| res.is_none());

        assert!(
            all_correct,
            "During the parsing of the faulty addresses, there was an incorrectness"
        );
    }
}
