use crate::behaviour::{ExpiredExternalAddr, FromSwarm, NewExternalAddr};
use crate::IntoConnectionHandler;
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
    pub fn on_swarn_event<THandler>(&mut self, event: &FromSwarm<THandler>)
    where
        THandler: IntoConnectionHandler,
    {
        match event {
            FromSwarm::NewExternalAddr(NewExternalAddr { addr, .. }) => {
                if self.addresses.len() < self.limit {
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
