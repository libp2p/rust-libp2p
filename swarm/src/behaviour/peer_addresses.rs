use crate::behaviour::FromSwarm;
use crate::{DialError, DialFailure, NewExternalAddrOfPeer};

use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;

use lru::LruCache;

use std::collections::HashSet;
use std::num::NonZeroUsize;

/// Struct for tracking peers' external addresses of the [`Swarm`](crate::Swarm).
#[derive(Debug)]
pub struct PeerAddresses(LruCache<PeerId, HashSet<Multiaddr>>);

impl PeerAddresses {
    /// Feed a [`FromSwarm`] event to this struct.
    ///
    /// Returns whether the event changed peer's known external addresses.
    pub fn on_swarm_event(&mut self, event: &FromSwarm) -> bool {
        match event {
            FromSwarm::NewExternalAddrOfPeer(NewExternalAddrOfPeer { peer_id, addr }) => {
                if let Some(peer_addrs) = self.get_mut(peer_id) {
                    let addr = Self::prepare_addr(peer_id, addr);
                    return peer_addrs.insert(addr);
                } else {
                    let addr = Self::prepare_addr(peer_id, addr);
                    self.put(*peer_id, std::iter::once(addr));
                    return true;
                }
            }
            FromSwarm::DialFailure(DialFailure { peer_id, error, .. }) => {
                if let Some(peer_id) = peer_id {
                    match error {
                        DialError::NoAddresses => return self.0.pop(peer_id).is_some(),
                        _ => return false,
                    }
                }
            }
            _ => return false,
        };
        false
    }

    pub fn new(cache_size: NonZeroUsize) -> Self {
        Self(LruCache::new(cache_size))
    }

    pub fn get_mut(&mut self, peer: &PeerId) -> Option<&mut HashSet<Multiaddr>> {
        self.0.get_mut(peer)
    }

    pub fn put(&mut self, peer: PeerId, addresses: impl Iterator<Item = Multiaddr>) -> bool {
        let addresses = addresses.filter_map(|a| a.with_p2p(peer).ok());
        self.0.put(peer, HashSet::from_iter(addresses));
        true
    }

    /// Returns peer's external addresses.
    pub fn get(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.0
            .get(peer)
            .cloned()
            .map(Vec::from_iter)
            .unwrap_or_default()
    }

    fn prepare_addr(peer: &PeerId, addr: &Multiaddr) -> Multiaddr {
        let addr = addr.clone();
        addr.clone().with_p2p(*peer).unwrap_or(addr)
    }
}

impl Default for PeerAddresses {
    fn default() -> Self {
        Self(LruCache::new(NonZeroUsize::new(100).unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{ConnectionId, DialError};

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

        let addr1 = MEMORY_ADDR_1000.clone().with_p2p(peer_id).unwrap();
        assert_eq!(cache.get(&peer_id), vec![addr1]);

        let event = new_external_addr_of_peer2(peer_id);
        let changed = cache.on_swarm_event(&event);

        let addr1 = MEMORY_ADDR_1000.clone().with_p2p(peer_id).unwrap();
        let addr2 = MEMORY_ADDR_2000.clone().with_p2p(peer_id).unwrap();

        assert_eq!(cache.get(&peer_id).sort(), vec![addr1, addr2].sort());
        assert!(changed);
    }

    #[test]
    fn existing_addr_is_not_added_to_cache() {
        let mut cache = PeerAddresses::default();
        let peer_id = PeerId::random();

        let event = new_external_addr_of_peer1(peer_id);

        let addr1 = MEMORY_ADDR_1000.clone().with_p2p(peer_id).unwrap();
        let changed = cache.on_swarm_event(&event);
        assert!(changed);
        assert_eq!(cache.get(&peer_id), vec![addr1]);

        let addr1 = MEMORY_ADDR_1000.clone().with_p2p(peer_id).unwrap();
        let changed = cache.on_swarm_event(&event);
        assert!(!changed);
        assert_eq!(cache.get(&peer_id), vec![addr1]);
    }

    #[test]
    fn addrs_of_peer_are_removed_when_received_dial_failure_event() {
        let mut cache = PeerAddresses::default();
        let peer_id = PeerId::random();

        cache.on_swarm_event(&new_external_addr_of_peer1(peer_id));
        let event = dial_error(peer_id);

        let changed = cache.on_swarm_event(&event);

        assert!(changed);
        assert_eq!(cache.get(&peer_id), vec![]);
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

    fn dial_error(peer_id: PeerId) -> FromSwarm<'static> {
        FromSwarm::DialFailure(DialFailure {
            peer_id: Some(peer_id),
            error: &DialError::NoAddresses,
            connection_id: ConnectionId::new_unchecked(8),
        })
    }

    static MEMORY_ADDR_1000: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(1000)));
    static MEMORY_ADDR_2000: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(2000)));
}
