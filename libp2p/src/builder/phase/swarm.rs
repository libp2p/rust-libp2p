use super::*;
use crate::SwarmBuilder;
use libp2p_swarm::{NetworkBehaviour, Swarm};
use std::marker::PhantomData;

pub struct SwarmPhase<T, B> {
    pub(crate) behaviour: B,
    pub(crate) transport: T,
}

macro_rules! impl_with_swarm_config {
    ($providerKebabCase:literal, $providerPascalCase:ty, $config:expr) => {
        #[cfg(feature = $providerKebabCase)]
        impl<T, B> SwarmBuilder<$providerPascalCase, SwarmPhase<T, B>> {
            pub fn with_swarm_config(
                self,
                constructor: impl FnOnce(libp2p_swarm::Config) -> libp2p_swarm::Config,
            ) -> SwarmBuilder<$providerPascalCase, BuildPhase<T, B>> {
                SwarmBuilder {
                    phase: BuildPhase {
                        behaviour: self.phase.behaviour,
                        transport: self.phase.transport,
                        swarm_config: constructor($config),
                    },
                    keypair: self.keypair,
                    phantom: PhantomData,
                }
            }
        }
    };
}

impl_with_swarm_config!(
    "async-std",
    super::provider::AsyncStd,
    libp2p_swarm::Config::with_async_std_executor()
);

impl_with_swarm_config!(
    "tokio",
    super::provider::Tokio,
    libp2p_swarm::Config::with_tokio_executor()
);

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
