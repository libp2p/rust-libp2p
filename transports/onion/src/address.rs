use std::net::{IpAddr, SocketAddr};

use arti_client::{IntoTorAddr, TorAddr, TorAddrError};
use libp2p_core::{multiaddr::Protocol, Multiaddr};

pub fn dangerous_extract_tor_address(multiaddr: &Multiaddr) -> Result<TorAddr, TorAddrError> {
    let socket_addr = try_extract_socket_addr(&mut multiaddr.clone())?;
    TorAddr::dangerously_from(socket_addr)
}

pub fn safe_extract_tor_address(multiaddr: &mut Multiaddr) -> Result<TorAddr, TorAddrError> {
    let tcp_variant = multiaddr.pop().ok_or(TorAddrError::NoPort)?;
    let tcp_port = match tcp_variant {
        Protocol::Tcp(p) => p,
        _ => return Err(TorAddrError::NoPort),
    };
    let dns_variant = multiaddr.pop().ok_or(TorAddrError::InvalidHostname)?;
    let dns_addr = match dns_variant {
        Protocol::Dns(dns) => dns,
        Protocol::Dns4(dns) => dns,
        Protocol::Dns6(dns) => dns,
        _ => return Err(TorAddrError::InvalidHostname),
    };
    let address_tuple = (dns_addr.as_ref(), tcp_port);
    address_tuple.into_tor_addr()
}

fn try_extract_socket_addr(multiaddr: &mut Multiaddr) -> Result<SocketAddr, TorAddrError> {
    let tcp_variant = multiaddr.pop().ok_or(TorAddrError::NoPort)?;
    let port = match tcp_variant {
        Protocol::Tcp(p) => p,
        _ => return Err(TorAddrError::NoPort),
    };
    let ip_variant = multiaddr.pop().ok_or(TorAddrError::InvalidHostname)?;
    let ip = match ip_variant {
        Protocol::Ip4(ip4) => IpAddr::V4(ip4),
        Protocol::Ip6(ip6) => IpAddr::V6(ip6),
        _ => return Err(TorAddrError::InvalidHostname),
    };
    Ok(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use std::ops::Not;

    use super::safe_extract_tor_address;

    #[test]
    fn extract_correct_address() {
        let mut addresses = [
            "/dns/ip.tld/tcp/10".parse().unwrap(),
            "/dns4/dns.ip4.tld/tcp/11".parse().unwrap(),
            "/dns6/dns.ip6.tld/tcp/12".parse().unwrap(),
        ];

        let all_correct = addresses
            .iter_mut()
            .map(safe_extract_tor_address)
            .all(|extract_res| extract_res.is_ok() && extract_res.unwrap().is_ip_address().not());

        assert!(all_correct, "Not all addresses have been parsed correctly");
    }

    #[test]
    fn detect_incorrect_address() {
        let mut addresses = [
            "/tcp/10/udp/12".parse().unwrap(),
            "/dns/ip.tld/dns4/ip.tld/dns6/ip.tld".parse().unwrap(),
            "/tcp/10/ip4/1.1.1.1".parse().unwrap(),
        ];

        let all_correct = addresses
            .iter_mut()
            .map(safe_extract_tor_address)
            .all(|res| res == Err(arti_client::TorAddrError::NoPort));

        assert!(
            all_correct,
            "During the parsing of the faulty addresses, there was an incorrectness"
        );
    }
}
