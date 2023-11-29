use crate::{behaviour::FromSwarm, NewExternalAddrOfPeer};
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use std::collections::HashMap;

/// Struct for tracking peers' external addresses of the [`Swarm`](crate::Swarm).
#[derive(Debug, Clone, Default)]
pub struct PeerAddresses(HashMap<PeerId, Vec<Multiaddr>>);

impl PeerAddresses {
    /// Feed a [`FromSwarm`] event to this struct.
    ///
    /// Returns whether the event changed peer's known external addresses.
    pub fn on_swarm_event(&mut self, event: &FromSwarm) -> bool {
        if let FromSwarm::NewExternalAddrOfPeer(NewExternalAddrOfPeer { peer_id, addr }) = event {
            match self.0.get_mut(peer_id) {
                None => {
                    let mut empty_vec: Vec<Multiaddr> = Vec::new();
                    Self::push(&mut empty_vec, addr);

                    self.0.insert(*peer_id, empty_vec);
                    return true;
                }
                Some(addrs) => {
                    if addrs.contains(addr) {
                        return false;
                    }
                    Self::push(addrs, addr);

                    return true;
                }
            }
        }
        false
    }

    /// Returns peer's external addresses.
    /// Returns [`None`] if the peer did not have any addrs cached.
    pub fn peer_addrs(&self, peer_id: &PeerId) -> Option<&Vec<Multiaddr>> {
        self.0.get(peer_id)
    }

    fn push(vec: &mut Vec<Multiaddr>, addr: &Multiaddr) {
        vec.insert(0, addr.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::multiaddr::Protocol;
    use once_cell::sync::Lazy;

    #[test]
    fn new_peer_addr_returns_correct_changed_value() {
        let mut cache = PeerAddresses::default();
        let peer_id = PeerId::random();

        let event = new_external_addr_of_peer1(peer_id);

        let changed = cache.on_swarm_event(&event);
        assert!(changed);

        let changed = cache.on_swarm_event(&event);
        assert!(!changed);
    }

    #[test]
    fn new_peer_addr_saves_peer_addrs() {
        let mut cache = PeerAddresses::default();
        let peer_id = PeerId::random();
        let event = new_external_addr_of_peer1(peer_id);

        let changed = cache.on_swarm_event(&event);
        assert!(changed);

        let addr1 = MEMORY_ADDR_1000.clone();
        assert_eq!(cache.peer_addrs(&peer_id), Some(&vec![addr1]));

        let event = new_external_addr_of_peer2(peer_id);
        let changed = cache.on_swarm_event(&event);

        let addr1 = MEMORY_ADDR_1000.clone();
        let addr2 = MEMORY_ADDR_2000.clone();

        assert_eq!(cache.peer_addrs(&peer_id), Some(&vec![addr2, addr1]));
        assert!(changed);
    }

    #[test]
    fn existing_addr_is_not_added_to_cache() {
        let mut cache = PeerAddresses::default();
        let peer_id = PeerId::random();

        let event = new_external_addr_of_peer1(peer_id);

        let addr1 = MEMORY_ADDR_1000.clone();
        let changed = cache.on_swarm_event(&event);
        assert!(changed);
        assert_eq!(cache.peer_addrs(&peer_id), Some(&vec![addr1]));

        let addr1 = MEMORY_ADDR_1000.clone();
        let changed = cache.on_swarm_event(&event);
        assert!(!changed);
        assert_eq!(cache.peer_addrs(&peer_id), Some(&vec![addr1]));
    }

    fn new_external_addr_of_peer1(peer_id: PeerId) -> FromSwarm<'static> {
        FromSwarm::NewExternalAddrOfPeer(NewExternalAddrOfPeer {
            peer_id,
            addr: &MEMORY_ADDR_1000,
        })
    }

    fn new_external_addr_of_peer2(peer_id: PeerId) -> FromSwarm<'static> {
        FromSwarm::NewExternalAddrOfPeer(NewExternalAddrOfPeer {
            peer_id,
            addr: &MEMORY_ADDR_2000,
        })
    }

    static MEMORY_ADDR_1000: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(1000)));
    static MEMORY_ADDR_2000: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(2000)));
}
