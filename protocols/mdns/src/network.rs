// Copyright 2020 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use if_addrs::IfAddr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::{collections::HashSet, io};

// Network related helper for the mdns implementation.

// Make set_multicast_if_v4 available on asynchronous sockets:
#[cfg(feature = "async-std")]
/// Set [IP_MULTICAST_IF](https://www.man7.org/linux/man-pages/man7/ip.7.html) for an async socket.
pub fn set_multicast_if_v4(
    socket: &async_std::net::UdpSocket,
    interface: &Ipv4Addr,
) -> std::io::Result<()> {
    #[cfg(not(windows))]
    use async_std::os::unix::io::AsRawFd;
    use net2::UdpSocketExt;
    use std::net::UdpSocket;
    #[cfg(not(windows))]
    use std::os::unix::io::FromRawFd;
    #[cfg(not(windows))]
    use std::os::unix::io::IntoRawFd;
    // Temporary unsafe double ownership:
    #[cfg(windows)]
    let std_sock: UdpSocket = unsafe { FromRawSocket::from_raw_socket(socket.as_raw_socket()) };
    #[cfg(not(windows))]
    let std_sock: UdpSocket = unsafe { FromRawFd::from_raw_fd(socket.as_raw_fd()) };
    // The unsafe sections above are safe under the assumption that the following two lines of
    // code won't panic.
    // Don't use ? here, we need to drop the ownership in the end!
    let r = std_sock.set_multicast_if_v4(interface);
    // Drop ownership again, thus avoid double free!
    std_sock.into_raw_fd();
    r
}

// Make set_multicast_if_v4 available on asynchronous sockets:
#[cfg(feature = "tokio")]
pub fn set_multicast_if_v4_tokio(
    socket: &tokio::net::UdpSocket,
    interface: &Ipv4Addr,
) -> std::io::Result<()> {
    use net2::UdpSocketExt;
    use std::net::UdpSocket;
    #[cfg(not(windows))]
    use std::os::unix::io::AsRawFd;
    #[cfg(not(windows))]
    use std::os::unix::io::FromRawFd;
    #[cfg(not(windows))]
    use std::os::unix::io::IntoRawFd;
    // Temporary unsafe double ownership:
    #[cfg(windows)]
    let std_sock: UdpSocket = unsafe { FromRawSocket::from_raw_socket(socket.as_raw_socket()) };
    #[cfg(not(windows))]
    let std_sock: UdpSocket = unsafe { FromRawFd::from_raw_fd(socket.as_raw_fd()) };
    // The unsafe sections above are safe under the assumption that the following two lines of
    // code won't panic.
    // Don't use ? here, we need to drop the ownership in the end!
    let r = std_sock.set_multicast_if_v4(interface);
    // Drop ownership again, thus avoid double free!
    std_sock.into_raw_fd();
    r
}

/// Get IPv4 addresses of all external network interfaces.
///
/// To avoid double broad casting with all its issues (see
/// [rfc6762](https://tools.ietf.org/html/rfc6762#page-42)), we only pick one interface for a any
/// given non link-local network, see `get_unique_interfaces`. This should ensure good operation in
/// almost all configurations. The only exception is multiple link local addresses bridged to the
/// same network, which needs to be tackled in the way described in the RFC.
pub fn get_interface_addresses() -> io::Result<impl Iterator<Item = Ipv4Addr>> {
    // Ok(if_addrs::get_if_addrs()?
    Ok(get_unique_interfaces()?
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(addr) => Some(addr),
            _ => None,
        }))
}
/// Get all unique interfaces.
///
/// This means all interfaces which do not belong to the same subnet as some other interface. For each subnet only one interface will be returned (except for link local addresses).
///
/// Note: If one interface belongs to a smaller subnetwork which is part of the network of another
/// interface, the implementation might or might not return both, depending on whether the masking
/// with their corresponding netmask results in the same base network or not.
///
/// Having a single machine with one interface being in a network which is a subnetwork of the
/// network of another interface is a weird enough configuration to make this inconsistent
/// behaviour justifyable and keeps the implementation simple.
///
/// For more details on the exact behaviour see the unit test `test_get_interface_network_v4`.
fn get_unique_interfaces() -> io::Result<impl Iterator<Item = if_addrs::Interface>> {
    let mut seen_networks = HashSet::new();
    let mut result = Vec::new();
    for i in if_addrs::get_if_addrs()? {
        let network = get_interface_network(&i);
        if ip_is_link_local(&network) || !seen_networks.contains(&network) {
            seen_networks.insert(network);
            result.push(i);
        }
    }
    Ok(result.into_iter())
}

/// Get the network of an interface
///
/// by computing the logical bitwise conjunction between the address and the subnetmask.
fn get_interface_network(interface: &if_addrs::Interface) -> IpAddr {
    // Very redundant code, but operations on arrays are rather limited right now,
    // see: https://internals.rust-lang.org/t/collecting-iterators-into-arrays/10330
    // and https://github.com/rust-lang/rust/issues/44580
    match &interface.addr {
        IfAddr::V4(addr) => {
            let mut result: [u8; 4] = [0; 4];
            let ip_octets = addr.ip.octets();
            let net_octets = addr.netmask.octets();
            let elements: _ = ip_octets
                .iter()
                .zip(net_octets.iter())
                .zip(result.iter_mut());
            for ((addr, mask), result) in elements {
                *result = addr & mask;
            }
            From::from(result)
        }
        IfAddr::V6(addr) => {
            let mut result: [u8; 16] = [0; 16];
            let ip_octets = addr.ip.octets();
            let net_octets = addr.netmask.octets();
            let elements = ip_octets
                .iter()
                .zip(net_octets.iter())
                .zip(result.iter_mut());
            for ((addr, mask), result) in elements {
                *result = addr & mask;
            }
            From::from(result)
        }
    }
}

/// Helper for checking link local IPv6 and IPv4 addresses
fn ip_is_link_local(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr) => addr.is_link_local(),
        IpAddr::V6(addr) => ip_is_unicast_link_local_strict(&addr),
    }
}

// Copied over
// [ip_is_unicast_link_local_strict](https://doc.rust-lang.org/nightly/std/net/struct.Ipv6Addr.html#method.is_unicast_link_local_strict) from the standard library (it has not yet been
// stabilized).
pub fn ip_is_unicast_link_local_strict(addr: &Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xffff) == 0xfe80
        && (addr.segments()[1] & 0xffff) == 0
        && (addr.segments()[2] & 0xffff) == 0
        && (addr.segments()[3] & 0xffff) == 0
}

#[cfg(test)]
mod test {
    use super::*;
    use if_addrs::{IfAddr, Ifv4Addr, Ifv6Addr, Interface};
    #[test]
    fn test_get_interface_network_v4() {
        let network1_a = Interface {
            name: "gibberish".to_string(),
            addr: IfAddr::V4(Ifv4Addr {
                ip: Ipv4Addr::new(192, 168, 129, 10),
                netmask: Ipv4Addr::new(255, 255, 128, 0),
                broadcast: None,
            }),
        };
        // Other interface in the same network
        let network1_b = Interface {
            name: "gibberish2".to_string(),
            addr: IfAddr::V4(Ifv4Addr {
                ip: Ipv4Addr::new(192, 168, 130, 22),
                netmask: Ipv4Addr::new(255, 255, 128, 0),
                broadcast: None,
            }),
        };
        // Other interface in a different network
        let network2_a = Interface {
            name: "gibberish3".to_string(),
            addr: IfAddr::V4(Ifv4Addr {
                ip: Ipv4Addr::new(192, 168, 127, 22),
                netmask: Ipv4Addr::new(255, 255, 128, 0),
                broadcast: None,
            }),
        };
        // Same ip, but smaller netmask.
        let network2_big_a = Interface {
            name: "gibberish4".to_string(),
            addr: IfAddr::V4(Ifv4Addr {
                ip: Ipv4Addr::new(192, 168, 127, 22),
                netmask: Ipv4Addr::new(255, 255, 0, 0),
                broadcast: None,
            }),
        };
        assert_eq!(
            get_interface_network(&network1_a),
            IpAddr::V4(Ipv4Addr::new(192, 168, 128, 0))
        );
        assert_eq!(
            get_interface_network(&network1_a),
            get_interface_network(&network1_b)
        );
        assert_ne!(
            get_interface_network(&network1_a),
            get_interface_network(&network2_a)
        );
        // Different netmask, but masking returns the same IP:
        assert_eq!(
            get_interface_network(&network2_a),
            get_interface_network(&network2_big_a)
        );
        // Different netmask and masking returns different IP:
        assert_ne!(
            get_interface_network(&network1_a),
            get_interface_network(&network2_big_a)
        );
        assert_ne!(
            get_interface_network(&network1_b),
            get_interface_network(&network2_big_a)
        );
    }

    #[test]
    fn test_get_interface_network_v6() {
        let network1_a = Interface {
            name: "gibberish".to_string(),
            addr: IfAddr::V6(Ifv6Addr {
                ip: Ipv6Addr::new(10, 0, 0, 0, 0, 0, 0, 1),
                netmask: Ipv6Addr::new(0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0, 0, 0),
                broadcast: None,
            }),
        };
        // Other interface in the same network
        let network1_b = Interface {
            name: "gibberish2".to_string(),
            addr: IfAddr::V6(Ifv6Addr {
                ip: Ipv6Addr::new(10, 0, 0, 0, 0, 0, 0, 2),
                netmask: Ipv6Addr::new(0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0, 0, 0),
                broadcast: None,
            }),
        };
        // Other interface in a different network
        let network2_a = Interface {
            name: "gibberish3".to_string(),
            addr: IfAddr::V6(Ifv6Addr {
                ip: Ipv6Addr::new(10, 0, 0, 0, 10, 0, 0, 2),
                netmask: Ipv6Addr::new(0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0, 0, 0),
                broadcast: None,
            }),
        };
        // Same ip, but smaller netmask.
        let network2_big_a = Interface {
            name: "gibberish4".to_string(),
            addr: IfAddr::V6(Ifv6Addr {
                ip: Ipv6Addr::new(10, 0, 0, 0, 10, 0, 0, 2),
                netmask: Ipv6Addr::new(0xffff, 0xffff, 0xffff, 0xffff, 0, 0, 0, 0),
                broadcast: None,
            }),
        };
        assert_eq!(
            get_interface_network(&network1_a),
            IpAddr::V6(Ipv6Addr::new(10, 0, 0, 0, 0, 0, 0, 0))
        );
        assert_eq!(
            get_interface_network(&network1_a),
            get_interface_network(&network1_b)
        );
        assert_ne!(
            get_interface_network(&network1_a),
            get_interface_network(&network2_a)
        );
        // Different netmask and masking returns different IP:
        assert_ne!(
            get_interface_network(&network2_a),
            get_interface_network(&network2_big_a)
        );
        // Different netmask, but masking returns the same IP:
        assert_eq!(
            get_interface_network(&network1_a),
            get_interface_network(&network2_big_a)
        );
        assert_eq!(
            get_interface_network(&network1_b),
            get_interface_network(&network2_big_a)
        );
    }
}
