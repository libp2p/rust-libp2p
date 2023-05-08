// Copyright 2023 Protocol Labs
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

use crate::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
};
use log::debug;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

/// Dropping all dial requests to non-global IP addresses.
#[derive(Debug, Clone, Default)]
pub struct Transport<T> {
    inner: T,
}

mod ipv4_global {
    use std::net::Ipv4Addr;

    /// Returns [`true`] if this address part of the `0.0.0.0/8` range.
    const fn is_this_network(a: Ipv4Addr) -> bool {
        a.octets()[0] == 0
    }

    /// Returns [`true`] if this address part of the `0.0.0.0/32` range.
    const fn is_this_host_on_this_network(a: Ipv4Addr) -> bool {
        a.octets()[0] == 0 && a.octets()[1] == 0 && a.octets()[2] == 0 && a.octets()[3] == 0
    }

    /// Returns [`true`] if this address is part of the Shared Address Space defined in
    /// [IETF RFC 6598] (`100.64.0.0/10`).
    ///
    /// [IETF RFC 6598]: https://tools.ietf.org/html/rfc6598
    const fn is_shared(a: Ipv4Addr) -> bool {
        a.octets()[0] == 100 && (a.octets()[1] & 0b1100_0000 == 0b0100_0000)
    }

    /// Returns [`true`] if this address is reserved by IANA for future use. [IETF RFC 1112]
    /// defines the block of reserved addresses as `240.0.0.0/4`. This range normally includes the
    /// broadcast address `255.255.255.255`, but this implementation explicitly excludes it, since
    /// it is obviously not reserved for future use.
    ///
    /// [IETF RFC 1112]: https://tools.ietf.org/html/rfc1112
    const fn is_reserved(a: Ipv4Addr) -> bool {
        a.octets()[0] & 240 == 240 && !a.is_broadcast()
    }

    /// Returns [`true`] if this address part of the `198.18.0.0/15` range, which is reserved for
    /// network devices benchmarking. This range is defined in [IETF RFC 2544] as `192.18.0.0`
    /// through `198.19.255.255` but [errata 423] corrects it to `198.18.0.0/15`.
    ///
    /// [IETF RFC 2544]: https://tools.ietf.org/html/rfc2544
    /// [errata 423]: https://www.rfc-editor.org/errata/eid423
    const fn is_benchmarking(a: Ipv4Addr) -> bool {
        a.octets()[0] == 198 && (a.octets()[1] & 0xfe) == 18
    }

    /// Returns [`true`] if this address part of the `192.0.0.0/29` range.
    const fn is_ipv4_service_continuity_prefix(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192
            && a.octets()[1] == 0
            && a.octets()[2] == 0
            && (a.octets()[3] & 0b1111_1000 == 0)
    }

    /// Returns [`true`] if this address part of the `192.0.0.0/24` range.
    const fn is_ietf_protocol_assignments(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 0 && a.octets()[2] == 0
    }

    /// Returns [`true`] if this address part of the `192.0.0.8/32` range.
    const fn is_ipv4_dummy_address(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 0 && a.octets()[2] == 0 && a.octets()[3] == 8
    }

    /// Returns [`true`] if this address part of the `192.0.0.9/32` range.
    const fn is_port_control_protocol_anycast(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 0 && a.octets()[2] == 0 && a.octets()[3] == 9
    }

    /// Returns [`true`] if this address part of the `192.0.0.10/32` range.
    const fn is_traversal_using_relays_around_nat_anycast(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 0 && a.octets()[2] == 0 && a.octets()[3] == 10
    }

    /// Returns [`true`] if this is NAT64/DNS64 Discovery. This includes:
    ///
    ///  - `192.0.0.170/32`
    ///  - `192.0.0.171/32`
    const fn is_nat64_dns64_discovery(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192
            && a.octets()[1] == 0
            && a.octets()[2] == 0
            && (a.octets()[3] == 170 || a.octets()[3] == 171)
    }

    /// Returns [`true`] if this address part of the `192.31.196.0/24` range.
    const fn is_as112_v4(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 31 && a.octets()[1] == 196
    }

    /// Returns [`true`] if this address part of the `192.52.193.0/24` range.
    const fn is_amt(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 52 && a.octets()[2] == 193
    }

    /// Returns [`true`] if this address part of the `192.88.99.0/24` range.
    const fn is_deprecated_6to6_relay_anycast(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 88 && a.octets()[2] == 99
    }

    /// Returns [`true`] if this address part of the `192.175.48.0/24` range.
    const fn is_direct_delegation_as112_service(a: Ipv4Addr) -> bool {
        a.octets()[0] == 192 && a.octets()[1] == 175 && a.octets()[2] == 48
    }

    /// The function checks if an IPv4 address is a global address by verifying that it does not belong
    /// to any of the reserved or special use address ranges.
    ///
    /// Arguments:
    ///
    /// * `a`: `a` is an `Ipv4Addr` type variable representing an IPv4 address. The function
    /// `is_global` checks whether the given IPv4 address is a global IP address or not by checking
    /// against various criteria such as private IP address ranges, reserved IP address ranges, and
    /// special use
    ///
    /// Returns:
    ///
    /// A boolean value indicating whether the given IPv4 address is a global address or not.
    pub fn is_global(a: Ipv4Addr) -> bool {
        !(is_this_network(a)
            || is_this_host_on_this_network(a)
            || a.is_private()
            || is_shared(a)
            || a.is_loopback()
            || a.is_link_local()
            || is_ietf_protocol_assignments(a)
            || is_ipv4_service_continuity_prefix(a)
            || is_ipv4_dummy_address(a)
            || is_port_control_protocol_anycast(a)
            || is_traversal_using_relays_around_nat_anycast(a)
            || is_nat64_dns64_discovery(a)
            || a.is_documentation()
            || is_as112_v4(a)
            || is_amt(a)
            || is_deprecated_6to6_relay_anycast(a)
            || is_direct_delegation_as112_service(a))
            || is_benchmarking(a)
            || is_reserved(a)
            || a.is_broadcast()
    }
}

mod ipv6_global {
    use std::net::Ipv6Addr;

    /// Returns [`true`] if this address is `::ffff:0:0/96`.
    const fn is_ipv4_mapped_address(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0, 0, 0, 0, 0, 0xffff, _, _])
    }

    /// Returns [`true`] if this address is `64:ff9b::/96` or `64:ff9b:1::/48`.
    const fn is_ipv4_ipv6_translat(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x64, 0xff9b, 0, 0, 0, 0, _, _])
            || matches!(a.segments(), [0x64, 0xff9b, 1, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `100::/64`.
    const fn is_discard_only_address_block(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x100, 0, 0, 0, _, _, _, _])
    }

    /// Returns [`true`] if this address is `2001::/23`.
    const fn is_ietf_protocol_assignments(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, b, _, _, _, _, _, _] if b <= 0x1FF)
    }

    /// Returns [`true`] if this address is `2001::/32`.
    const fn is_teredo(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, 0, _, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `2001:1::1/128`.
    const fn is_port_control_protocol_anycast(a: Ipv6Addr) -> bool {
        u128::from_be_bytes(a.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0001
    }

    /// Returns [`true`] if this address is `2001:1::2/128`.
    const fn is_traversal_relays_nat_anycast(a: Ipv6Addr) -> bool {
        u128::from_be_bytes(a.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0002
    }

    /// Returns [`true`] if this address is `2001:2::/48`.
    const fn is_benchmarking(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, 2, 0, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `2001:3::/32`.
    const fn is_amt(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, 3, _, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `2001:4:112::/48`.
    const fn is_as112_v6(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, 4, 0x112, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `2001:10::/28`.
    const fn is_deprecated_previously_orchid(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, b, _, _, _, _, _, _] if b >= 0x10 && b <= 0x1F)
    }

    /// Returns [`true`] if this address is `2001:20::/28`.
    const fn is_orchid_v2(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, b, _, _, _, _, _, _] if b >= 0x20 && b <= 0x2F)
    }

    /// Returns [`true`] if this address is `2001:30::/28`.
    const fn is_drone_remote(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, b, _, _, _, _, _, _] if b >= 0x30 && b <= 0x3F)
    }

    /// Returns [`true`] if this address is `2001:db8::/32`.
    const fn is_documentation(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2001, 0xdb8, _, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `2002::/16`.
    const fn is_6to4(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2002, _, _, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `2620:4f:8000::/48`.
    const fn is_direct_delegation_as112_service(a: Ipv6Addr) -> bool {
        matches!(a.segments(), [0x2620, 0x4f, 0x8000, _, _, _, _, _])
    }

    /// Returns [`true`] if this address is `fc00::/7`.
    const fn is_unique_local(a: Ipv6Addr) -> bool {
        (a.segments()[0] & 0xfe00) == 0xfc00
    }

    /// Returns [`true`] if this address is `fe80::/10`.
    const fn is_unicast_link_local(a: Ipv6Addr) -> bool {
        (a.segments()[0] & 0xffc0) == 0xfe80
    }

    pub fn is_global(a: Ipv6Addr) -> bool {
        !(a.is_loopback()
            || a.is_unspecified()
            || is_ipv4_mapped_address(a)
            || is_ipv4_ipv6_translat(a)
            || is_discard_only_address_block(a)
            || is_ietf_protocol_assignments(a)
            || is_teredo(a)
            || is_port_control_protocol_anycast(a)
            || is_traversal_relays_nat_anycast(a)
            || is_benchmarking(a)
            || is_amt(a)
            || is_as112_v6(a)
            || is_deprecated_previously_orchid(a)
            || is_orchid_v2(a)
            || is_drone_remote(a)
            || is_documentation(a)
            || is_6to4(a)
            || is_direct_delegation_as112_service(a)
            || is_unique_local(a)
            || is_unicast_link_local(a))
    }
}

impl<T> Transport<T> {
    pub fn new(transport: T) -> Self {
        Transport { inner: transport }
    }
}

impl<T: crate::Transport + Unpin> crate::Transport for Transport<T> {
    type Output = <T as crate::Transport>::Output;
    type Error = <T as crate::Transport>::Error;
    type ListenerUpgrade = <T as crate::Transport>::ListenerUpgrade;
    type Dial = <T as crate::Transport>::Dial;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        self.inner.listen_on(addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        match addr.iter().next() {
            Some(Protocol::Ip4(a)) => {
                if !ipv4_global::is_global(a) {
                    debug!("Not dialing non global IP address {:?}.", a);
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
                self.inner.dial(addr)
            }
            Some(Protocol::Ip6(a)) => {
                if !ipv6_global::is_global(a) {
                    debug!("Not dialing non global IP address {:?}.", a);
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
                self.inner.dial(addr)
            }
            _ => {
                debug!("Not dialing unsupported Multiaddress {:?}.", addr);
                Err(TransportError::MultiaddrNotSupported(addr))
            }
        }
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        match addr.iter().next() {
            Some(Protocol::Ip4(a)) => {
                if !ipv4_global::is_global(a) {
                    debug!("Not dialing non global IP address {:?}.", a);
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
                self.inner.dial_as_listener(addr)
            }
            Some(Protocol::Ip6(a)) => {
                if !ipv6_global::is_global(a) {
                    debug!("Not dialing non global IP address {:?}.", a);
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
                self.inner.dial_as_listener(addr)
            }
            _ => {
                debug!("Not dialing unsupported Multiaddress {:?}.", addr);
                Err(TransportError::MultiaddrNotSupported(addr))
            }
        }
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(listen, observed)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Pin::new(&mut self.inner).poll(cx)
    }
}
