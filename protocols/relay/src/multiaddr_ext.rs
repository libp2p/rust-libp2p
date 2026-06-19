use libp2p_core::{Multiaddr, multiaddr::Protocol};
use libp2p_identity::PeerId;

pub(crate) trait MultiaddrExt {
    fn is_relayed(&self) -> bool;
}

impl MultiaddrExt for Multiaddr {
    fn is_relayed(&self) -> bool {
        self.iter().any(|p| p == Protocol::P2pCircuit)
    }
}

pub(crate) fn relay_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    let mut last_p2p = None;
    for proto in addr.iter() {
        match proto {
            Protocol::P2p(peer) => last_p2p = Some(peer),
            Protocol::P2pCircuit => return last_p2p,
            _ => {}
        }
    }
    None
}