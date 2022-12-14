use crate::behaviour::{ExpiredListenAddr, FromSwarm, NewListenAddr};
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
    pub fn on_swarm_event<THandler>(&mut self, event: &FromSwarm<THandler>)
    where
        THandler: IntoConnectionHandler,
    {
        match event {
            FromSwarm::NewListenAddr(NewListenAddr { addr, .. }) => {
                self.addresses.insert((*addr).clone());
            }
            FromSwarm::ExpiredListenAddr(ExpiredListenAddr { addr, .. }) => {
                self.addresses.insert((*addr).clone());
            }
            _ => {}
        }
    }
}
