use std::{marker::PhantomData, sync::Arc};

use super::*;
#[allow(deprecated)]
use crate::bandwidth::BandwidthSinks;
use crate::{transport_ext::TransportExt, SwarmBuilder};

pub struct BandwidthLoggingPhase<T, R> {
    pub(crate) relay_behaviour: R,
    pub(crate) transport: T,
}

impl<T: AuthenticatedMultiplexedTransport, Provider, R>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, R>>
{
    #[allow(deprecated)]
    #[deprecated(note = "Use `with_bandwidth_metrics` instead.")]
    pub fn with_bandwidth_logging(
        self,
    ) -> (
        SwarmBuilder<Provider, BandwidthMetricsPhase<impl AuthenticatedMultiplexedTransport, R>>,
        Arc<BandwidthSinks>,
    ) {
        let (transport, sinks) = self.phase.transport.with_bandwidth_logging();
        (
            SwarmBuilder {
                phase: BandwidthMetricsPhase {
                    relay_behaviour: self.phase.relay_behaviour,
                    transport,
                },
                keypair: self.keypair,
                phantom: PhantomData,
            },
            sinks,
        )
    }

    pub fn without_bandwidth_logging(self) -> SwarmBuilder<Provider, BandwidthMetricsPhase<T, R>> {
        SwarmBuilder {
            phase: BandwidthMetricsPhase {
                relay_behaviour: self.phase.relay_behaviour,
                transport: self.phase.transport,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

// Shortcuts
#[cfg(feature = "metrics")]
impl<Provider, T: AuthenticatedMultiplexedTransport, R>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, R>>
{
    pub fn with_bandwidth_metrics(
        self,
        registry: &mut libp2p_metrics::Registry,
    ) -> SwarmBuilder<Provider, BehaviourPhase<impl AuthenticatedMultiplexedTransport, R>> {
        self.without_bandwidth_logging()
            .with_bandwidth_metrics(registry)
    }
}
#[cfg(feature = "relay")]
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, libp2p_relay::client::Behaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_logging()
            .without_bandwidth_metrics()
            .with_behaviour(constructor)
    }
}
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, NoRelayBehaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_logging()
            .without_bandwidth_metrics()
            .with_behaviour(constructor)
    }
}
