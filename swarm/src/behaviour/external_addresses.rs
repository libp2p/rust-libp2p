use crate::behaviour::{ExternalAddrConfirmed, ExternalAddrExpired, FromSwarm};
use libp2p_core::Multiaddr;

/// The maximum number of local external addresses. When reached any
/// further externally reported addresses are ignored. The behaviour always
/// tracks all its listen addresses.
const MAX_LOCAL_EXTERNAL_ADDRS: usize = 20;

/// Utility struct for tracking the external addresses of a [`Swarm`](crate::Swarm).
#[derive(Debug, Clone, Default)]
pub struct ExternalAddresses {
    addresses: Vec<Multiaddr>,
}

impl ExternalAddresses {
    /// Returns an [`Iterator`] over all external addresses.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Multiaddr> {
        self.addresses.iter()
    }

    pub fn as_slice(&self) -> &[Multiaddr] {
        self.addresses.as_slice()
    }

    /// Feed a [`FromSwarm`] event to this struct.
    ///
    /// Returns whether the event changed our set of external addresses.
    pub fn on_swarm_event<THandler>(&mut self, event: &FromSwarm<THandler>) -> bool {
        match event {
            FromSwarm::ExternalAddrConfirmed(ExternalAddrConfirmed { addr }) => {
                if self.addresses.contains(addr) {
                    return false;
                }

                if self.addresses.len() >= MAX_LOCAL_EXTERNAL_ADDRS {
                    // If we hit the limit, we can add the address in O(1):
                    self.addresses.push((*addr).clone());
                    self.addresses.swap_remove(0);

                    return true;
                }

                self.addresses.insert(0, (*addr).clone()); // We have at most `MAX_LOCAL_EXTERNAL_ADDRS` so this isn't very expensive.

                return true;
            }
            FromSwarm::ExternalAddrExpired(ExternalAddrExpired { addr: expired_addr, .. }) => {
                let pos = match self.addresses.iter().position(|candidate| candidate == *expired_addr) {
                    None => return false,
                    Some(p) => p,
                };

                self.addresses.remove(pos);
                return true;
            }
            _ => {}
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dummy;
    use libp2p_core::multiaddr::Protocol;
    use once_cell::sync::Lazy;
    use rand::Rng;

    #[test]
    fn new_external_addr_returns_correct_changed_value() {
        let mut addresses = ExternalAddresses::default();

        let changed = addresses.on_swarm_event(&new_external_addr1());
        assert!(changed);

        let changed = addresses.on_swarm_event(&new_external_addr1());
        assert!(!changed)
    }

    #[test]
    fn expired_external_addr_returns_correct_changed_value() {
        let mut addresses = ExternalAddresses::default();
        addresses.on_swarm_event(&new_external_addr1());

        let changed = addresses.on_swarm_event(&expired_external_addr1());
        assert!(changed);

        let changed = addresses.on_swarm_event(&expired_external_addr1());
        assert!(!changed)
    }

    #[test]
    fn more_recent_external_addresses_are_prioritized() {
        let mut addresses = ExternalAddresses::default();

        addresses.on_swarm_event(&new_external_addr1());
        addresses.on_swarm_event(&new_external_addr2());

        assert_eq!(addresses.as_slice(), &[(*MEMORY_ADDR_2000).clone(), (*MEMORY_ADDR_1000).clone()]);
    }

    #[test]
    fn when_pushing_more_than_max_addresses_oldest_is_evicted() {
        let mut addresses = ExternalAddresses::default();

        for _ in 0..MAX_LOCAL_EXTERNAL_ADDRS {
            let random_address = Multiaddr::empty().with(Protocol::Memory(rand::thread_rng().gen_range(0..1000)));
            addresses.on_swarm_event(&FromSwarm::<'_, dummy::ConnectionHandler>::ExternalAddrConfirmed(ExternalAddrConfirmed { addr: &random_address }));
        }

        addresses.on_swarm_event(&new_external_addr2());

        assert_eq!(addresses.as_slice().len(), 20);
        assert_eq!(addresses.as_slice()[0], (*MEMORY_ADDR_2000).clone());
    }

    fn new_external_addr1() -> FromSwarm<'static, dummy::ConnectionHandler> {
        FromSwarm::ExternalAddrConfirmed(ExternalAddrConfirmed { addr: &MEMORY_ADDR_1000 })
    }

    fn new_external_addr2() -> FromSwarm<'static, dummy::ConnectionHandler> {
        FromSwarm::ExternalAddrConfirmed(ExternalAddrConfirmed { addr: &MEMORY_ADDR_2000 })
    }

    fn expired_external_addr1() -> FromSwarm<'static, dummy::ConnectionHandler> {
        FromSwarm::ExternalAddrExpired(ExternalAddrExpired { addr: &MEMORY_ADDR_1000 })
    }

    static MEMORY_ADDR_1000: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(1000)));
    static MEMORY_ADDR_2000: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(2000)));
}
