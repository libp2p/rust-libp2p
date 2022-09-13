use std::net::{IpAddr, SocketAddr};

use arti_client::{TorAddr, TorAddrError};
use libp2p_core::{multiaddr::Protocol, Multiaddr};

fn try_extract_socket_addr(mutliaddr: &Multiaddr) -> Result<SocketAddr, TorAddrError> {
    let mut ip_4 = None;
    let mut ip_6 = None;
    let mut tcp_port_opt = None;
    for e in mutliaddr.iter() {
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
