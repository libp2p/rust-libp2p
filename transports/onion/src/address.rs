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
    use std::{borrow::Cow, net::Ipv4Addr, ops::Not};

    use libp2p_core::{multiaddr::multiaddr, Multiaddr};

    use super::safe_extract_tor_address;

    #[test]
    fn extract_correct_address() {
        let dns_test_addr = multiaddr!(Dns(Cow::Borrowed("ip.tld")), Tcp(10u16));
        let dns_test_addr_res = safe_extract_tor_address(&mut dns_test_addr.clone());
        assert!(dns_test_addr_res.is_ok());
        assert!(dns_test_addr_res.unwrap().is_ip_address().not());

        let dns_4_test_addr = multiaddr!(Dns4(Cow::Borrowed("dns.ip4.tld")), Tcp(11u16));
        let dns_4_test_addr_res = safe_extract_tor_address(&mut dns_4_test_addr.clone());
        assert!(dns_4_test_addr_res.is_ok());
        assert!(dns_4_test_addr_res.unwrap().is_ip_address().not());

        let dns_6_test_addr = multiaddr!(Dns6(Cow::Borrowed("dns.ip6.tld")), Tcp(12u16));
        let dns_6_test_addr_res = safe_extract_tor_address(&mut dns_6_test_addr.clone());
        assert!(dns_6_test_addr_res.is_ok());
        assert!(dns_6_test_addr_res.unwrap().is_ip_address().not());

        let addresses = [
            dns_test_addr,
            dns_4_test_addr,
            dns_6_test_addr,
            Multiaddr::empty(),
        ];

        for (idx, a) in addresses.iter().enumerate() {
            for (idy, b) in addresses.iter().enumerate() {
                if idx == idy {
                    continue;
                }
                let mut new = Multiaddr::empty();
                a.iter().for_each(|e| {
                    new.push(e);
                });
                b.iter().for_each(|e| {
                    new.push(e);
                });
                let new_addr_res = safe_extract_tor_address(&mut new);
                assert!(new_addr_res.is_ok());
            }
        }
    }

    #[test]
    fn detect_incorrect_address() {
        let without_dns = multiaddr!(Tcp(10u16), Udp(12u16));
        let without_dns_res = safe_extract_tor_address(&mut without_dns.clone());
        assert_eq!(
            without_dns_res,
            Err(arti_client::TorAddrError::NoPort)
        );

        let host = Cow::Borrowed("ip.tld");
        let without_port = multiaddr!(Dns(host.clone()), Dns4(host.clone()), Dns6(host.clone()));
        let without_port_res = safe_extract_tor_address(&mut without_port.clone());
        assert_eq!(without_port_res, Err(arti_client::TorAddrError::NoPort));
        let with_ip_addr = multiaddr!(Tcp(10u16), Ip4("1.1.1.1".parse::<Ipv4Addr>().unwrap()));
        let with_ip_addr_res = safe_extract_tor_address(&mut with_ip_addr.clone());
        assert_eq!(
            with_ip_addr_res,
            Err(arti_client::TorAddrError::NoPort)
        );
    }
}
