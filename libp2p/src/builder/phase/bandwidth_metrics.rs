use super::*;
use crate::bandwidth::BandwidthSinks;
use crate::transport_ext::TransportExt;
use crate::SwarmBuilder;
use std::marker::PhantomData;
use std::sync::Arc;

pub struct BandwidthMetricsPhase<T, R> {
    pub(crate) relay_behaviour: R,
    pub(crate) transport: T,
}

#[cfg(feature = "metrics")]
impl<T: AuthenticatedMultiplexedTransport, Provider, R>
    SwarmBuilder<Provider, BandwidthMetricsPhase<T, R>>
{
    pub fn with_bandwidth_metrics(
        self,
        registry: &mut libp2p_metrics::Registry,
    ) -> SwarmBuilder<Provider, BehaviourPhase<impl AuthenticatedMultiplexedTransport, R>> {
        SwarmBuilder {
            phase: BehaviourPhase {
                relay_behaviour: self.phase.relay_behaviour,
                transport: libp2p_metrics::bandwidth::Transport::new(
                    self.phase.transport,
                    registry,
                ),
            },
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    pub fn without_bandwidth_metrics(self) -> SwarmBuilder<Provider, BehaviourPhase<T, R>> {
        SwarmBuilder {
            phase: BehaviourPhase {
                relay_behaviour: self.phase.relay_behaviour,
                transport: self.phase.transport,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

// Shortcuts
#[cfg(feature = "relay")]
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, BandwidthMetricsPhase<T, libp2p_relay::client::Behaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_metrics().with_behaviour(constructor)
    }
}

impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, BandwidthMetricsPhase<T, NoRelayBehaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_metrics().with_behaviour(constructor)
    }
}
