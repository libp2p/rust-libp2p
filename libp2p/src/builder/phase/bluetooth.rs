use std::marker::PhantomData;

use super::*;
use crate::SwarmBuilder;

use libp2p_core::upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade};
use libp2p_core::{Negotiated, UpgradeInfo};

pub struct BluetoothPhase<T> {
    pub(crate) transport: T,
}

impl<Provider, T> SwarmBuilder<Provider, BluetoothPhase<T>> {
    pub(crate) fn without_bluetooth(self) -> SwarmBuilder<Provider, OtherTransportPhase<T>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: OtherTransportPhase {
                transport: self.phase.transport,
            },
        }
    }
}

impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, BluetoothPhase<T>> {
    pub fn with_other_transport<
        Muxer: libp2p_core::muxing::StreamMuxer + Send + 'static,
        OtherTransport: Transport<Output = (libp2p_identity::PeerId, Muxer)> + Send + Unpin + 'static,
        R: TryIntoTransport<OtherTransport>,
    >(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<
        SwarmBuilder<Provider, OtherTransportPhase<impl AuthenticatedMultiplexedTransport>>,
        R::Error,
    >
    where
        <OtherTransport as Transport>::Error: Send + Sync + 'static,
        <OtherTransport as Transport>::Dial: Send,
        <OtherTransport as Transport>::ListenerUpgrade: Send,
        <Muxer as libp2p_core::muxing::StreamMuxer>::Substream: Send,
        <Muxer as libp2p_core::muxing::StreamMuxer>::Error: Send + Sync,
    {
        self.without_bluetooth().with_other_transport(constructor)
    }

    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bluetooth()
            .without_any_other_transports()
            .without_dns()
            .without_websocket()
            .without_relay()
            .with_behaviour(constructor)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::Tokio, BluetoothPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<
            super::provider::Tokio,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport>,
        >,
        std::io::Error,
    > {
        self.without_bluetooth()
            .without_any_other_transports()
            .with_dns()
    }

    pub fn with_dns_config(
        self,
        cfg: libp2p_dns::ResolverConfig,
        opts: libp2p_dns::ResolverOpts,
    ) -> SwarmBuilder<super::provider::Tokio, WebsocketPhase<impl AuthenticatedMultiplexedTransport>>
    {
        self.without_bluetooth()
            .without_any_other_transports()
            .with_dns_config(cfg, opts)
    }
}
