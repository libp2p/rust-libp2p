use super::*;
use crate::SwarmBuilder;
use libp2p_core::Transport;
use libp2p_swarm::Swarm;

pub struct BuildPhase<T, B> {
    pub(crate) behaviour: B,
    pub(crate) transport: T,
    pub(crate) swarm_config: libp2p_swarm::Config,
}

const CONNECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl<Provider, T: AuthenticatedMultiplexedTransport, B: libp2p_swarm::NetworkBehaviour>
    SwarmBuilder<Provider, BuildPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        Swarm::new(
            libp2p_core::transport::timeout::TransportTimeout::new(
                self.phase.transport,
                CONNECTION_TIMEOUT,
            )
            .boxed(),
            self.phase.behaviour,
            self.keypair.public().to_peer_id(),
            self.phase.swarm_config,
        )
    }
}
