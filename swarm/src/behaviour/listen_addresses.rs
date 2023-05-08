use crate::behaviour::{ExpiredListenAddr, FromSwarm, NewListenAddr};
#[allow(deprecated)]
use crate::IntoConnectionHandler;
use libp2p_core::Multiaddr;
use std::collections::HashSet;

/// Utility struct for tracking the addresses a [`Swarm`](crate::Swarm) is listening on.
#[derive(Debug, Default, Clone)]
pub struct ListenAddresses {
    addresses: HashSet<Multiaddr>,
}

impl ListenAddresses {
    /// Returns an [`Iterator`] over all listen addresses.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Multiaddr> {
        self.addresses.iter()
    }

    /// Feed a [`FromSwarm`] event to this struct.
    ///
    /// Returns whether the event changed our set of listen addresses.
    #[allow(deprecated)]
    pub fn on_swarm_event<THandler>(&mut self, event: &FromSwarm<THandler>) -> bool
    where
        THandler: IntoConnectionHandler,
    {
        match event {
            FromSwarm::NewListenAddr(NewListenAddr { addr, .. }) => {
                self.addresses.insert((*addr).clone())
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr { addr, .. }) => {
                self.addresses.remove(addr)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dummy;
    use libp2p_core::multiaddr::Protocol;
    use once_cell::sync::Lazy;

    #[test]
    fn new_listen_addr_returns_correct_changed_value() {
        let mut addresses = ListenAddresses::default();

        let changed = addresses.on_swarm_event(&new_listen_addr());
        assert!(changed);

        let changed = addresses.on_swarm_event(&new_listen_addr());
        assert!(!changed)
    }

    #[test]
    fn expired_listen_addr_returns_correct_changed_value() {
        let mut addresses = ListenAddresses::default();
        addresses.on_swarm_event(&new_listen_addr());

        let changed = addresses.on_swarm_event(&expired_listen_addr());
        assert!(changed);

        let changed = addresses.on_swarm_event(&expired_listen_addr());
        assert!(!changed)
    }

    fn new_listen_addr() -> FromSwarm<'static, dummy::ConnectionHandler> {
        FromSwarm::NewListenAddr(NewListenAddr {
            listener_id: Default::default(),
            addr: &MEMORY_ADDR,
        })
    }

    fn expired_listen_addr() -> FromSwarm<'static, dummy::ConnectionHandler> {
        FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id: Default::default(),
            addr: &MEMORY_ADDR,
        })
    }

    static MEMORY_ADDR: Lazy<Multiaddr> =
        Lazy::new(|| Multiaddr::empty().with(Protocol::Memory(1000)));
}
