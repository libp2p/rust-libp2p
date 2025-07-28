use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;

/// Creates a listener multiaddress for the local machine with the specified TCP port.
pub(crate) fn get_listener_addr(port: u16) -> Result<Multiaddr, &'static str> {
    let mut addr = get_local_multiaddr()?;
    addr.push(Protocol::Tcp(port));
    Ok(addr)
}

/// Gets the local machine's non-loopback IPv4 address as a multiaddress.
fn get_local_multiaddr() -> Result<Multiaddr, &'static str> {
    let Ok(ifs) = if_addrs::get_if_addrs() else {
        return Err("Failed to get interfaces.");
    };

    let addrs = ifs
        .iter()
        .filter(|i| !i.addr.is_loopback() && i.addr.ip().is_ipv4())
        .map(|i| Multiaddr::try_from(i.addr.ip()).unwrap())
        .collect::<Vec<_>>();

    if addrs.len() > 1 {
        return Err("Failed to get interfaces.");
    }

    Ok(addrs[0].clone())
}
