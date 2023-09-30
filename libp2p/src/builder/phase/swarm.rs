use super::*;
use crate::SwarmBuilder;
use libp2p_swarm::{NetworkBehaviour, Swarm};
use std::marker::PhantomData;

pub struct SwarmPhase<T, B> {
    pub(crate) behaviour: B,
    pub(crate) transport: T,
}

impl<T, B, Provider> SwarmBuilder<Provider, SwarmPhase<T, B>> {
    pub fn with_swarm_config(
        self,
        config: libp2p_swarm::Config,
    ) -> SwarmBuilder<Provider, BuildPhase<T, B>> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: config,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "async-std"))]
impl<T: AuthenticatedMultiplexedTransport, B: NetworkBehaviour>
    SwarmBuilder<super::provider::AsyncStd, SwarmPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: libp2p_swarm::Config::with_async_std_executor(),
            },
            keypair: self.keypair,
            phantom: PhantomData::<super::provider::AsyncStd>,
        }
        .build()
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
impl<T: AuthenticatedMultiplexedTransport, B: NetworkBehaviour>
    SwarmBuilder<super::provider::Tokio, SwarmPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: libp2p_swarm::Config::with_tokio_executor(),
            },
            keypair: self.keypair,
            phantom: PhantomData::<super::provider::Tokio>,
        }
        .build()
    }
}

#[cfg(feature = "wasm-bindgen")]
impl<T: AuthenticatedMultiplexedTransport, B: NetworkBehaviour>
    SwarmBuilder<super::provider::WasmBindgen, SwarmPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: libp2p_swarm::Config::with_wasm_executor(),
            },
            keypair: self.keypair,
            phantom: PhantomData::<super::provider::WasmBindgen>,
        }
        .build()
    }
}
