use std::net::{IpAddr, SocketAddr};

use arti_client::{IntoTorAddr, TorAddr, TorAddrError};
use libp2p_core::{multiaddr::Protocol, Multiaddr};

fn try_extract_socket_addr(multiaddr: &Multiaddr) -> Result<SocketAddr, TorAddrError> {
    let mut ip_4 = None;
    let mut ip_6 = None;
    let mut tcp_port_opt = None;
    for e in multiaddr.iter() {
        match e {
            Protocol::Ip4(a) => {
                ip_4 = Some(IpAddr::V4(a));
            }
            Protocol::Ip6(a) => {
                ip_6 = Some(IpAddr::V6(a));
            }
            Protocol::Tcp(p) => {
                tcp_port_opt = Some(p);
            }
            _ => {}
        }
    }
    let ip = ip_4.or(ip_6).ok_or(TorAddrError::InvalidHostname)?;
    let tcp_port = tcp_port_opt.ok_or(TorAddrError::NoPort)?;
    Ok(SocketAddr::new(ip, tcp_port))
}

pub(super) fn dangerous_extract_tor_address(
    multiaddr: &Multiaddr,
) -> Result<TorAddr, TorAddrError> {
    let socket_addr = try_extract_socket_addr(multiaddr)?;
    TorAddr::dangerously_from(socket_addr)
}

macro_rules! try_convert_to_tor_addr {
    ($dns:ident, $tcp_port:ident, $tor_addr_error:ident) => {
        if let Some(dns_s) = $dns {
            match (dns_s.as_ref(), $tcp_port).into_tor_addr() {
                Ok(tor_addr) => return Ok(tor_addr),
                Err(e) => $tor_addr_error = Some(e),
            }
        }
    };
}

pub(super) fn safe_extract_tor_address(multiaddr: &Multiaddr) -> Result<TorAddr, TorAddrError> {
    let mut dns = None;
    let mut dns_4 = None;
    let mut dns_6 = None;
    let mut dns_addr = None;
    let mut tcp_port_opt = None;
    for e in multiaddr.iter() {
        match e {
            Protocol::Dns(s) => {
                dns = Some(s);
            }
            Protocol::Dns4(s) => {
                dns_4 = Some(s);
            }
            Protocol::Dns6(s) => {
                dns_6 = Some(s);
            }
            Protocol::Dnsaddr(s) => {
                dns_addr = Some(s);
            }
            Protocol::Tcp(p) => {
                tcp_port_opt = Some(p);
            }
            _ => {}
        }
    }
    let tcp_port = tcp_port_opt.ok_or(TorAddrError::NoPort)?;
    let mut tor_addr_error = None;
    try_convert_to_tor_addr!(dns, tcp_port, tor_addr_error);
    try_convert_to_tor_addr!(dns_4, tcp_port, tor_addr_error);
    try_convert_to_tor_addr!(dns_6, tcp_port, tor_addr_error);
    try_convert_to_tor_addr!(dns_addr, tcp_port, tor_addr_error);
    Err(tor_addr_error.unwrap_or(TorAddrError::InvalidHostname))
}
