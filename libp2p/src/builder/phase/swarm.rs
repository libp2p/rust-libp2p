#[allow(unused_imports)]
use super::*;

#[allow(unused)] // used below but due to feature flag combinations, clippy gives an unnecessary warning.
const DEFAULT_CONNECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[allow(dead_code)]
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

#[cfg(not(target_arch = "wasm32"))]
impl_with_swarm_config!(
    "async-std",
    super::provider::AsyncStd,
    libp2p_swarm::Config::with_async_std_executor()
);

#[cfg(not(target_arch = "wasm32"))]
impl_with_swarm_config!(
    "tokio",
    super::provider::Tokio,
    libp2p_swarm::Config::with_tokio_executor()
);

#[cfg(target_arch = "wasm32")]
impl_with_swarm_config!(
    "wasm-bindgen",
    super::provider::WasmBindgen,
    libp2p_swarm::Config::with_wasm_executor()
);
