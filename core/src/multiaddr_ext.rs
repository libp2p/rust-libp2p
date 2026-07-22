use crate::multiaddr::{Multiaddr, Protocol};

/// Extension trait providing convenience helpers for [`Multiaddr`].
pub trait MultiaddrExt {
    /// Returns `true` if the address is relayed, i.e. it contains a
    /// [`p2p-circuit`](Protocol::P2pCircuit) component.
    fn is_relayed(&self) -> bool;
}

impl MultiaddrExt for Multiaddr {
    fn is_relayed(&self) -> bool {
        self.iter().any(|p| p == Protocol::P2pCircuit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_address_is_not_relayed() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/1234".parse().unwrap();
        assert!(!addr.is_relayed());
    }

    #[test]
    fn circuit_address_is_relayed() {
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/1234/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN/p2p-circuit/p2p/12D3KooWGzBRs8WVcMaEXGWwsvFSfJKYphwM6ojrPanuJfup1xFz".parse().unwrap();
        assert!(addr.is_relayed());
    }
}
