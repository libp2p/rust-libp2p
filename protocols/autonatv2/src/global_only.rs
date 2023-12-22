use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub(crate) trait Ipv4Ext {
    /// Returns [`true`] if this address is reserved by IANA for future use. [IETF RFC 1112]
    /// defines the block of reserved addresses as `240.0.0.0/4`. This range normally includes the
    /// broadcast address `255.255.255.255`, but this implementation explicitly excludes it, since
    /// it is obviously not reserved for future use.
    ///
    /// [IETF RFC 1112]: https://tools.ietf.org/html/rfc1112
    ///
    /// # Warning
    ///
    /// As IANA assigns new addresses, this method will be
    /// updated. This may result in non-reserved addresses being
    /// treated as reserved in code that relies on an outdated version
    /// of this method.
    #[must_use]
    fn is_reserved(&self) -> bool;
    /// Returns [`true`] if this address part of the `198.18.0.0/15` range, which is reserved for
    /// network devices benchmarking. This range is defined in [IETF RFC 2544] as `192.18.0.0`
    /// through `198.19.255.255` but [errata 423] corrects it to `198.18.0.0/15`.
    ///
    /// [IETF RFC 2544]: https://tools.ietf.org/html/rfc2544
    /// [errata 423]: https://www.rfc-editor.org/errata/eid423
    #[must_use]
    fn is_benchmarking(&self) -> bool;
    /// Returns [`true`] if this address is part of the Shared Address Space defined in
    /// [IETF RFC 6598] (`100.64.0.0/10`).
    ///
    /// [IETF RFC 6598]: https://tools.ietf.org/html/rfc6598
    #[must_use]
    fn is_shared(&self) -> bool;
    /// Returns [`true`] if this is a private address.
    ///
    /// The private address ranges are defined in [IETF RFC 1918] and include:
    ///
    ///  - `10.0.0.0/8`
    ///  - `172.16.0.0/12`
    ///  - `192.168.0.0/16`
    ///
    /// [IETF RFC 1918]: https://tools.ietf.org/html/rfc1918
    #[must_use]
    fn is_private(&self) -> bool;
}

impl Ipv4Ext for Ipv4Addr {
    #[inline]
    fn is_reserved(&self) -> bool {
        self.octets()[0] & 240 == 240 && !self.is_broadcast()
    }
    #[inline]
    fn is_benchmarking(&self) -> bool {
        self.octets()[0] == 198 && (self.octets()[1] & 0xfe) == 18
    }
    #[inline]
    fn is_shared(&self) -> bool {
        self.octets()[0] == 100 && (self.octets()[1] & 0b1100_0000 == 0b0100_0000)
    }
    #[inline]
    fn is_private(&self) -> bool {
        match self.octets() {
            [10, ..] => true,
            [172, b, ..] if (16..=31).contains(&b) => true,
            [192, 168, ..] => true,
            _ => false,
        }
    }
}

pub(crate) trait Ipv6Ext {
    /// Returns `true` if the address is a unicast address with link-local scope,
    /// as defined in [RFC 4291].
    ///
    /// A unicast address has link-local scope if it has the prefix `fe80::/10`, as per [RFC 4291 section 2.4].
    /// Note that this encompasses more addresses than those defined in [RFC 4291 section 2.5.6],
    /// which describes "Link-Local IPv6 Unicast Addresses" as having the following stricter format:
    ///
    /// ```text
    /// | 10 bits  |         54 bits         |          64 bits           |
    /// +----------+-------------------------+----------------------------+
    /// |1111111010|           0             |       interface ID         |
    /// +----------+-------------------------+----------------------------+
    /// ```
    /// So while currently the only addresses with link-local scope an application will encounter are all in `fe80::/64`,
    /// this might change in the future with the publication of new standards. More addresses in `fe80::/10` could be allocated,
    /// and those addresses will have link-local scope.
    ///
    /// Also note that while [RFC 4291 section 2.5.3] mentions about the [loopback address] (`::1`) that "it is treated as having Link-Local scope",
    /// this does not mean that the loopback address actually has link-local scope and this method will return `false` on it.
    ///
    /// [RFC 4291]: https://tools.ietf.org/html/rfc4291
    /// [RFC 4291 section 2.4]: https://tools.ietf.org/html/rfc4291#section-2.4
    /// [RFC 4291 section 2.5.3]: https://tools.ietf.org/html/rfc4291#section-2.5.3
    /// [RFC 4291 section 2.5.6]: https://tools.ietf.org/html/rfc4291#section-2.5.6
    /// [loopback address]: Ipv6Addr::LOCALHOST
    #[must_use]
    fn is_unicast_link_local(&self) -> bool;
    /// Returns [`true`] if this is a unique local address (`fc00::/7`).
    ///
    /// This property is defined in [IETF RFC 4193].
    ///
    /// [IETF RFC 4193]: https://tools.ietf.org/html/rfc4193
    #[must_use]
    fn is_unique_local(&self) -> bool;
    /// Returns [`true`] if this is an address reserved for documentation
    /// (`2001:db8::/32`).
    ///
    /// This property is defined in [IETF RFC 3849].
    ///
    /// [IETF RFC 3849]: https://tools.ietf.org/html/rfc3849
    #[must_use]
    fn is_documentation(&self) -> bool;
}

impl Ipv6Ext for Ipv6Addr {
    #[inline]
    fn is_unicast_link_local(&self) -> bool {
        (self.segments()[0] & 0xffc0) == 0xfe80
    }

    #[inline]
    fn is_unique_local(&self) -> bool {
        (self.segments()[0] & 0xfe00) == 0xfc00
    }

    #[inline]
    fn is_documentation(&self) -> bool {
        (self.segments()[0] == 0x2001) && (self.segments()[1] == 0xdb8)
    }
}

pub(crate) trait IpExt {
    /// Returns [`true`] if the address appears to be globally routable.
    ///
    /// See the documentation for [`Ipv4Addr::is_global()`] and Ipv6Addr::is_global() for more details.
    #[must_use]
    fn is_global(&self) -> bool;
}

impl IpExt for Ipv4Addr {
    /// Returns [`true`] if the address appears to be globally reachable
    /// as specified by the [IANA IPv4 Special-Purpose Address Registry].
    /// Whether or not an address is practically reachable will depend on your network configuration.
    ///
    /// Most IPv4 addresses are globally reachable;
    /// unless they are specifically defined as *not* globally reachable.
    ///
    /// Non-exhaustive list of notable addresses that are not globally reachable:
    ///
    /// - The [unspecified address] ([`is_unspecified`](Ipv4Addr::is_unspecified))
    /// - Addresses reserved for private use ([`is_private`](Ipv4Addr::is_private))
    /// - Addresses in the shared address space ([`is_shared`](Ipv4Addr::is_shared))
    /// - Loopback addresses ([`is_loopback`](Ipv4Addr::is_loopback))
    /// - Link-local addresses ([`is_link_local`](Ipv4Addr::is_link_local))
    /// - Addresses reserved for documentation ([`is_documentation`](Ipv4Addr::is_documentation))
    /// - Addresses reserved for benchmarking ([`is_benchmarking`](Ipv4Addr::is_benchmarking))
    /// - Reserved addresses ([`is_reserved`](Ipv4Addr::is_reserved))
    /// - The [broadcast address] ([`is_broadcast`](Ipv4Addr::is_broadcast))
    ///
    /// For the complete overview of which addresses are globally reachable, see the table at the [IANA IPv4 Special-Purpose Address Registry].
    ///
    /// [IANA IPv4 Special-Purpose Address Registry]: https://www.iana.org/assignments/iana-ipv4-special-registry/iana-ipv4-special-registry.xhtml
    /// [unspecified address]: Ipv4Addr::UNSPECIFIED
    /// [broadcast address]: Ipv4Addr::BROADCAST
    #[inline]
    fn is_global(&self) -> bool {
        !(self.octets()[0] == 0 // "This network"
            || self.is_private()
            || Ipv4Ext::is_shared(self)
            || self.is_loopback()
            || self.is_link_local()
            // addresses reserved for future protocols (`192.0.0.0/24`)
            ||(self.octets()[0] == 192 && self.octets()[1] == 0 && self.octets()[2] == 0)
            || self.is_documentation()
            || Ipv4Ext::is_benchmarking(self)
            || Ipv4Ext::is_reserved(self)
            || self.is_broadcast())
    }
}

impl IpExt for Ipv6Addr {
    /// Returns [`true`] if the address appears to be globally reachable
    /// as specified by the [IANA IPv6 Special-Purpose Address Registry].
    /// Whether or not an address is practically reachable will depend on your network configuration.
    ///
    /// Most IPv6 addresses are globally reachable;
    /// unless they are specifically defined as *not* globally reachable.
    ///
    /// Non-exhaustive list of notable addresses that are not globally reachable:
    /// - The [unspecified address] ([`is_unspecified`](Ipv6Addr::is_unspecified))
    /// - The [loopback address] ([`is_loopback`](Ipv6Addr::is_loopback))
    /// - IPv4-mapped addresses
    /// - Addresses reserved for benchmarking
    /// - Addresses reserved for documentation ([`is_documentation`](Ipv6Addr::is_documentation))
    /// - Unique local addresses ([`is_unique_local`](Ipv6Addr::is_unique_local))
    /// - Unicast addresses with link-local scope ([`is_unicast_link_local`](Ipv6Addr::is_unicast_link_local))
    ///
    /// For the complete overview of which addresses are globally reachable, see the table at the [IANA IPv6 Special-Purpose Address Registry].
    ///
    /// Note that an address having global scope is not the same as being globally reachable,
    /// and there is no direct relation between the two concepts: There exist addresses with global scope
    /// that are not globally reachable (for example unique local addresses),
    /// and addresses that are globally reachable without having global scope
    /// (multicast addresses with non-global scope).
    ///
    /// [IANA IPv6 Special-Purpose Address Registry]: https://www.iana.org/assignments/iana-ipv6-special-registry/iana-ipv6-special-registry.xhtml
    /// [unspecified address]: Ipv6Addr::UNSPECIFIED
    /// [loopback address]: Ipv6Addr::LOCALHOST
    #[inline]
    fn is_global(&self) -> bool {
        !(self.is_unspecified()
            || self.is_loopback()
            // IPv4-mapped Address (`::ffff:0:0/96`)
            || matches!(self.segments(), [0, 0, 0, 0, 0, 0xffff, _, _])
            // IPv4-IPv6 Translat. (`64:ff9b:1::/48`)
            || matches!(self.segments(), [0x64, 0xff9b, 1, _, _, _, _, _])
            // Discard-Only Address Block (`100::/64`)
            || matches!(self.segments(), [0x100, 0, 0, 0, _, _, _, _])
            // IETF Protocol Assignments (`2001::/23`)
            || (matches!(self.segments(), [0x2001, b, _, _, _, _, _, _] if b < 0x200)
                && !(
                    // Port Control Protocol Anycast (`2001:1::1`)
                    u128::from_be_bytes(self.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0001
                    // Traversal Using Relays around NAT Anycast (`2001:1::2`)
                    || u128::from_be_bytes(self.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0002
                    // AMT (`2001:3::/32`)
                    || matches!(self.segments(), [0x2001, 3, _, _, _, _, _, _])
                    // AS112-v6 (`2001:4:112::/48`)
                    || matches!(self.segments(), [0x2001, 4, 0x112, _, _, _, _, _])
                    // ORCHIDv2 (`2001:20::/28`)
                    || matches!(self.segments(), [0x2001, b, _, _, _, _, _, _] if (0x20..=0x2f).contains(&b))
                ))
            || Ipv6Ext::is_documentation(self)
            || Ipv6Ext::is_unique_local(self)
            || Ipv6Ext::is_unicast_link_local(self))
    }
}

impl IpExt for IpAddr {
    #[inline]
    fn is_global(&self) -> bool {
        match self {
            Self::V4(v4) => IpExt::is_global(v4),
            Self::V6(v6) => IpExt::is_global(v6),
        }
    }
}
