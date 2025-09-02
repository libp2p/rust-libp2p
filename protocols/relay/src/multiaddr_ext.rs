use libp2p_core::{multiaddr::Protocol, Multiaddr};

pub(crate) trait MultiaddrExt {
    fn is_relayed(&self) -> bool;

    fn is_public(&self) -> bool;

    fn is_loopback(&self) -> bool;

    fn is_private(&self) -> bool;

    fn is_unspecified(&self) -> bool;
}

impl MultiaddrExt for Multiaddr {
    fn is_relayed(&self) -> bool {
        self.iter().any(|p| p == Protocol::P2pCircuit)
    }

    fn is_public(&self) -> bool {
        !self.is_private() && !self.is_loopback() && !self.is_unspecified()
    }

    fn is_loopback(&self) -> bool {
        self.iter().any(|proto| match proto {
            Protocol::Ip4(ip) => ip.is_loopback(),
            Protocol::Ip6(ip) => ip.is_loopback(),
            _ => false,
        })
    }

    fn is_private(&self) -> bool {
        self.iter().any(|proto| match proto {
            Protocol::Ip4(ip) => ip.is_private(),
            Protocol::Ip6(ip) => {
                (ip.segments()[0] & 0xffc0) != 0xfe80 && (ip.segments()[0] & 0xfe00) != 0xfc00
            }
            _ => false,
        })
    }

    fn is_unspecified(&self) -> bool {
        self.iter().any(|proto| match proto {
            Protocol::Ip4(ip) => ip.is_unspecified(),
            Protocol::Ip6(ip) => ip.is_unspecified(),
            _ => false,
        })
    }
}
