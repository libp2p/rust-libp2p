use crate::behaviour::{ExpiredExternalAddr, FromSwarm, NewExternalAddr};
use crate::IntoConnectionHandler;
use libp2p_core::Multiaddr;
use std::collections::HashSet;

/// Utility struct for tracking the external addresses of a [`Swarm`](crate::Swarm).
#[derive(Debug, Default, Clone)]
pub struct ExternalAddresses {
    addresses: HashSet<Multiaddr>,
    limit: Option<usize>,
}

impl ExternalAddresses {
    pub fn with_limit(max: usize) -> Self {
        Self {
            addresses: Default::default(),
            limit: Some(max),
        }
    }

    /// Returns an [`Iterator`] over all external addresses.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Multiaddr> {
        self.addresses.iter()
    }

    /// Feed a [`SwarmEvent`] to this struct.
    pub fn on_event<THandler>(&mut self, event: &FromSwarm<THandler>)
    where
        THandler: IntoConnectionHandler,
    {
        match event {
            FromSwarm::NewExternalAddr(NewExternalAddr { addr, .. }) => {
                if self.addresses.len() < self.limit.unwrap_or(usize::MAX) {
                    self.addresses.insert((*addr).clone());
                }
            }
            FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr { addr, .. }) => {
                self.addresses.insert((*addr).clone());
            }
            _ => {}
        }
    }
}
