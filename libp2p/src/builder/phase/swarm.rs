#[allow(unused_imports)]
use super::*;

#[allow(unused)] // used below but due to feature flag combinations, clippy gives an unnecessary warning.
const DEFAULT_CONNECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[allow(dead_code)]
pub struct SwarmPhase<T, B> {
    pub(crate) behaviour: B,
    pub(crate) transport: T,
}

#[cfg(any(target_arch = "wasm32", feature = "tokio"))]
macro_rules! impl_with_swarm_config {
    ($providerPascalCase:ty, $config:expr) => {
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
                        connection_timeout: DEFAULT_CONNECTION_TIMEOUT,
                    },
                    keypair: self.keypair,
                    phantom: std::marker::PhantomData,
                }
            }

            // Shortcuts
            pub fn build(self) -> libp2p_swarm::Swarm<B>
            where
                B: libp2p_swarm::NetworkBehaviour,
                T: AuthenticatedMultiplexedTransport,
            {
                self.with_swarm_config(std::convert::identity).build()
            }
        }
    };
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
impl_with_swarm_config!(
    super::provider::Tokio,
    libp2p_swarm::Config::with_tokio_executor()
);

#[cfg(target_arch = "wasm32")]
impl_with_swarm_config!(
    super::provider::WasmBindgen,
    libp2p_swarm::Config::with_wasm_executor()
);
