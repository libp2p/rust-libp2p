use libp2p_core::multiaddr::Protocol;
use libp2p_core::Multiaddr;

pub(crate) trait MultiaddrExt {
    fn is_relayed(&self) -> bool;
}

impl MultiaddrExt for Multiaddr {
    fn is_relayed(&self) -> bool {
        self.iter().any(|p| p == Protocol::P2pCircuit)
    }
}
