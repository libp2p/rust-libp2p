use crate::behaviour::{ExpiredExternalAddr, FromSwarm, NewExternalAddr};
use libp2p_core::Multiaddr;
use std::collections::HashSet;

/// The maximum number of local external addresses. When reached any
/// further externally reported addresses are ignored. The behaviour always
/// tracks all its listen addresses.
const MAX_LOCAL_EXTERNAL_ADDRS: usize = 20;

/// Utility struct for tracking the external addresses of a [`Swarm`](crate::Swarm).
#[derive(Debug, Clone)]
pub struct ExternalAddresses {
    addresses: HashSet<Multiaddr>,
    limit: usize,
}

impl Default for ExternalAddresses {
    fn default() -> Self {
        Self {
            addresses: Default::default(),
            limit: MAX_LOCAL_EXTERNAL_ADDRS,
        }
    }
}

impl ExternalAddresses {
    /// Returns an [`Iterator`] over all external addresses.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Multiaddr> {
        self.addresses.iter()
    }

    /// Feed a [`FromSwarm`] event to this struct.
    ///
    /// Returns whether the event changed our set of external addresses.
    pub fn on_swarm_event<THandler>(&mut self, event: &FromSwarm<THandler>) -> bool {
        match event {
            FromSwarm::NewExternalAddr(NewExternalAddr { addr, .. }) => {
                if self.addresses.len() < self.limit {
                    return self.addresses.insert((*addr).clone());
                }
            }
            FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr { addr, .. }) => {
                return self.addresses.remove(addr)
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

    #[test]
    fn new_external_addr_returns_correct_changed_value() {
        let mut addresses = ExternalAddresses::default();

        let changed = addresses.on_swarm_event(&new_external_addr());
        assert!(changed);

        let changed = addresses.on_swarm_event(&new_external_addr());
        assert!(!changed)
    }

    #[test]
    fn expired_external_addr_returns_correct_changed_value() {
        let mut addresses = ExternalAddresses::default();
        addresses.on_swarm_event(&new_external_addr());

        let changed = addresses.on_swarm_event(&expired_external_addr());
        assert!(changed);

        let changed = addresses.on_swarm_event(&expired_external_addr());
        assert!(!changed)
    }

    fn new_external_addr() -> FromSwarm<'static, dummy::ConnectionHandler> {
        FromSwarm::NewExternalAddr(NewExternalAddr { addr: &MEMORY_ADDR })
    }

    fn expired_external_addr() -> FromSwarm<'static, dummy::ConnectionHandler> {
        FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr { addr: &MEMORY_ADDR })
    }

    static MEMORY_ADDR: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(1000)));
}
