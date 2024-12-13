use std::marker::PhantomData;

use super::*;
use crate::SwarmBuilder;

pub struct DnsPhase<T> {
    pub(crate) transport: T,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::AsyncStd, DnsPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<
            super::provider::AsyncStd,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport>,
        >,
        std::io::Error,
    > {
        Ok(SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketPhase {
                transport: libp2p_dns::async_std::Transport::system2(self.phase.transport)?,
            },
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::Tokio, DnsPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<
            super::provider::Tokio,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport>,
        >,
        std::io::Error,
    > {
        Ok(SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketPhase {
                transport: libp2p_dns::tokio::Transport::system(self.phase.transport)?,
            },
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::AsyncStd, DnsPhase<T>> {
    pub fn with_dns_config(
        self,
        cfg: libp2p_dns::ResolverConfig,
        opts: libp2p_dns::ResolverOpts,
    ) -> SwarmBuilder<
        super::provider::AsyncStd,
        WebsocketPhase<impl AuthenticatedMultiplexedTransport>,
    > {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketPhase {
                transport: libp2p_dns::async_std::Transport::custom2(
                    self.phase.transport,
                    cfg,
                    opts,
                ),
            },
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::Tokio, DnsPhase<T>> {
    pub fn with_dns_config(
        self,
        cfg: libp2p_dns::ResolverConfig,
        opts: libp2p_dns::ResolverOpts,
    ) -> SwarmBuilder<super::provider::Tokio, WebsocketPhase<impl AuthenticatedMultiplexedTransport>>
    {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketPhase {
                transport: libp2p_dns::tokio::Transport::custom(self.phase.transport, cfg, opts),
            },
        }
    }
}

impl<Provider, T> SwarmBuilder<Provider, DnsPhase<T>> {
    pub(crate) fn without_dns(self) -> SwarmBuilder<Provider, WebsocketPhase<T>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketPhase {
                transport: self.phase.transport,
            },
        }
    }
}

// Shortcuts
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, DnsPhase<T>> {
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_dns()
            .without_websocket()
            .without_relay()
            .with_behaviour(constructor)
    }
}
