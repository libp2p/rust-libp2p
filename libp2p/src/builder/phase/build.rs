use std::time::Duration;

use libp2p_core::{transport::timeout::TransportTimeout, Transport};
use libp2p_swarm::Swarm;

#[allow(unused_imports)]
use super::*;
use crate::SwarmBuilder;

pub struct BuildPhase<T, B> {
    pub(crate) behaviour: B,
    pub(crate) transport: T,
    pub(crate) swarm_config: libp2p_swarm::Config,
    pub(crate) connection_timeout: Duration,
}

impl<Provider, T: AuthenticatedMultiplexedTransport, B: libp2p_swarm::NetworkBehaviour>
    SwarmBuilder<Provider, BuildPhase<T, B>>
{
    /// Timeout of the [`TransportTimeout`] wrapping the transport.
    pub fn with_connection_timeout(mut self, connection_timeout: Duration) -> Self {
        self.phase.connection_timeout = connection_timeout;
        self
    }

    pub fn build(self) -> Swarm<B> {
        Swarm::new(
            TransportTimeout::new(self.phase.transport, self.phase.connection_timeout).boxed(),
            self.phase.behaviour,
            self.keypair.public().to_peer_id(),
            self.phase.swarm_config,
        )
    }
}
