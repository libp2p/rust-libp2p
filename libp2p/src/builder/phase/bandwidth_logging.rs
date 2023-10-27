use multiaddr::Multiaddr;

use super::*;
use crate::metrics::bandwidth::{BandwidthSinks, Muxer};
use crate::transport_ext::TransportExt;
use crate::SwarmBuilder;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

pub struct BandwidthLoggingPhase<T, R> {
    pub(crate) relay_behaviour: R,
    pub(crate) transport: T,
}

#[cfg(feature = "metrics")]
impl<T: AuthenticatedMultiplexedTransport, Provider, R>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, R>>
{
    pub fn with_bandwidth_logging(
        self,
        registry: &mut libp2p_metrics::Registry,
    ) -> SwarmBuilder<Provider, BehaviourPhase<impl AuthenticatedMultiplexedTransport, R>> {
        SwarmBuilder {
            phase: BehaviourPhase {
                relay_behaviour: self.phase.relay_behaviour,
                transport: crate::metrics::bandwidth::Transport::new(self.phase.transport, registry),
            },
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

impl<T: AuthenticatedMultiplexedTransport, Provider, R>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, R>>
{
    pub fn without_bandwidth_logging(self) -> SwarmBuilder<Provider, BehaviourPhase<T, R>> {
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
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, libp2p_relay::client::Behaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_logging().with_behaviour(constructor)
    }
}

impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, NoRelayBehaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_logging().with_behaviour(constructor)
    }
}
