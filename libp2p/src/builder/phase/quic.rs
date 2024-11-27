use std::{marker::PhantomData, sync::Arc};

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_core::muxing::StreamMuxer;
use libp2p_core::upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade};
#[cfg(any(
    feature = "relay",
    all(not(target_arch = "wasm32"), feature = "websocket")
))]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, UpgradeInfo};

use super::*;
use crate::SwarmBuilder;

pub struct QuicPhase<T> {
    pub(crate) transport: T,
}

macro_rules! impl_quic_builder {
    ($providerKebabCase:literal, $providerPascalCase:ty, $quic:ident) => {
        #[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = $providerKebabCase))]
        impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<$providerPascalCase, QuicPhase<T>> {
            pub fn with_quic(
                self,
            ) -> SwarmBuilder<
                $providerPascalCase,
                OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
            > {
                self.with_quic_config(std::convert::identity)
            }

            pub fn with_quic_config(
                self,
                constructor: impl FnOnce(libp2p_quic::Config) -> libp2p_quic::Config,
            ) -> SwarmBuilder<
                $providerPascalCase,
                OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
            > {
                SwarmBuilder {
                    phase: OtherTransportPhase {
                        transport: self
                            .phase
                            .transport
                            .or_transport(
                                libp2p_quic::$quic::Transport::new(constructor(
                                    libp2p_quic::Config::new(&self.keypair),
                                ))
                                .map(|(peer_id, muxer), _| {
                                    (peer_id, libp2p_core::muxing::StreamMuxerBox::new(muxer))
                                }),
                            )
                            .map(|either, _| either.into_inner()),
                    },
                    keypair: self.keypair,
                    phantom: PhantomData,
                }
            }
        }
    };
}

impl_quic_builder!("async-std", AsyncStd, async_std);
impl_quic_builder!("tokio", super::provider::Tokio, tokio);

impl<Provider, T> SwarmBuilder<Provider, QuicPhase<T>> {
    pub(crate) fn without_quic(self) -> SwarmBuilder<Provider, OtherTransportPhase<T>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: OtherTransportPhase {
                transport: self.phase.transport,
            },
        }
    }
}

// Shortcuts
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, QuicPhase<T>> {
    /// See [`SwarmBuilder::with_relay_client`].
    #[cfg(feature = "relay")]
    pub fn with_relay_client<SecUpgrade, SecStream, SecError, MuxUpgrade, MuxStream, MuxError>(
        self,
        security_upgrade: SecUpgrade,
        multiplexer_upgrade: MuxUpgrade,
    ) -> Result<
        SwarmBuilder<
            Provider,
            BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
        >,
        SecUpgrade::Error,
        > where

        SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
        SecError: std::error::Error + Send + Sync + 'static,
        SecUpgrade: IntoSecurityUpgrade<libp2p_relay::client::Connection>,
        SecUpgrade::Upgrade: InboundConnectionUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + OutboundConnectionUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
    <SecUpgrade::Upgrade as InboundConnectionUpgrade<Negotiated<libp2p_relay::client::Connection>>>::Future: Send,
    <SecUpgrade::Upgrade as OutboundConnectionUpgrade<Negotiated<libp2p_relay::client::Connection>>>::Future: Send,
    <<<SecUpgrade as IntoSecurityUpgrade<libp2p_relay::client::Connection>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
    <<SecUpgrade as IntoSecurityUpgrade<libp2p_relay::client::Connection>>::Upgrade as UpgradeInfo>::Info: Send,

        MuxStream: libp2p_core::muxing::StreamMuxer + Send + 'static,
        MuxStream::Substream: Send + 'static,
        MuxStream::Error: Send + Sync + 'static,
        MuxUpgrade: IntoMultiplexerUpgrade<SecStream>,
        MuxUpgrade::Upgrade: InboundConnectionUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + OutboundConnectionUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + Clone + Send + 'static,
    <MuxUpgrade::Upgrade as InboundConnectionUpgrade<Negotiated<SecStream>>>::Future: Send,
    <MuxUpgrade::Upgrade as OutboundConnectionUpgrade<Negotiated<SecStream>>>::Future: Send,
        MuxError: std::error::Error + Send + Sync + 'static,
    <<<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
    <<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::Info: Send,
    {
        self.without_quic()
            .without_any_other_transports()
            .without_dns()
            .without_websocket()
            .with_relay_client(security_upgrade, multiplexer_upgrade)
    }

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
        self.without_quic().with_other_transport(constructor)
    }

    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_quic()
            .without_any_other_transports()
            .without_dns()
            .without_websocket()
            .without_relay()
            .with_behaviour(constructor)
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::AsyncStd, QuicPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<
            super::provider::AsyncStd,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport>,
        >,
        std::io::Error,
    > {
        self.without_quic()
            .without_any_other_transports()
            .with_dns()
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::Tokio, QuicPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<
            super::provider::Tokio,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport>,
        >,
        std::io::Error,
    > {
        self.without_quic()
            .without_any_other_transports()
            .with_dns()
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::AsyncStd, QuicPhase<T>> {
    pub fn with_dns_config(
        self,
        cfg: libp2p_dns::ResolverConfig,
        opts: libp2p_dns::ResolverOpts,
    ) -> SwarmBuilder<
        super::provider::AsyncStd,
        WebsocketPhase<impl AuthenticatedMultiplexedTransport>,
    > {
        self.without_quic()
            .without_any_other_transports()
            .with_dns_config(cfg, opts)
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::Tokio, QuicPhase<T>> {
    pub fn with_dns_config(
        self,
        cfg: libp2p_dns::ResolverConfig,
        opts: libp2p_dns::ResolverOpts,
    ) -> SwarmBuilder<super::provider::Tokio, WebsocketPhase<impl AuthenticatedMultiplexedTransport>>
    {
        self.without_quic()
            .without_any_other_transports()
            .with_dns_config(cfg, opts)
    }
}

macro_rules! impl_quic_phase_with_websocket {
    ($providerKebabCase:literal, $providerPascalCase:ty, $websocketStream:ty) => {
        #[cfg(all(feature = $providerKebabCase, not(target_arch = "wasm32"), feature = "websocket"))]
        impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<$providerPascalCase, QuicPhase<T>> {
            /// See [`SwarmBuilder::with_websocket`].
            pub async fn with_websocket <
                SecUpgrade,
                SecStream,
                SecError,
                MuxUpgrade,
                MuxStream,
                MuxError,
            > (
                self,
                security_upgrade: SecUpgrade,
                multiplexer_upgrade: MuxUpgrade,
            ) -> Result<
                    SwarmBuilder<
                        $providerPascalCase,
                        RelayPhase<impl AuthenticatedMultiplexedTransport>,
                    >,
                    super::websocket::WebsocketError<SecUpgrade::Error>,
                >
            where
                SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
                SecError: std::error::Error + Send + Sync + 'static,
                SecUpgrade: IntoSecurityUpgrade<$websocketStream>,
                SecUpgrade::Upgrade: InboundConnectionUpgrade<Negotiated<$websocketStream>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + OutboundConnectionUpgrade<Negotiated<$websocketStream>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
            <SecUpgrade::Upgrade as InboundConnectionUpgrade<Negotiated<$websocketStream>>>::Future: Send,
            <SecUpgrade::Upgrade as OutboundConnectionUpgrade<Negotiated<$websocketStream>>>::Future: Send,
            <<<SecUpgrade as IntoSecurityUpgrade<$websocketStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
            <<SecUpgrade as IntoSecurityUpgrade<$websocketStream>>::Upgrade as UpgradeInfo>::Info: Send,

                MuxStream: StreamMuxer + Send + 'static,
                MuxStream::Substream: Send + 'static,
                MuxStream::Error: Send + Sync + 'static,
                MuxUpgrade: IntoMultiplexerUpgrade<SecStream>,
                MuxUpgrade::Upgrade: InboundConnectionUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + OutboundConnectionUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + Clone + Send + 'static,
                <MuxUpgrade::Upgrade as InboundConnectionUpgrade<Negotiated<SecStream>>>::Future: Send,
                <MuxUpgrade::Upgrade as OutboundConnectionUpgrade<Negotiated<SecStream>>>::Future: Send,
                    MuxError: std::error::Error + Send + Sync + 'static,
                <<<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
                <<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::Info: Send,
            {
                self.without_quic()
                    .without_any_other_transports()
                    .without_dns()
                    .with_websocket(security_upgrade, multiplexer_upgrade)
                    .await
            }
        }
    }
}
impl_quic_phase_with_websocket!(
    "async-std",
    super::provider::AsyncStd,
    rw_stream_sink::RwStreamSink<
        libp2p_websocket::BytesConnection<libp2p_tcp::async_io::TcpStream>,
    >
);
impl_quic_phase_with_websocket!(
    "tokio",
    super::provider::Tokio,
    rw_stream_sink::RwStreamSink<libp2p_websocket::BytesConnection<libp2p_tcp::tokio::TcpStream>>
);
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, QuicPhase<T>> {
    #[allow(deprecated)]
    #[deprecated(note = "Use `with_bandwidth_metrics` instead.")]
    pub fn with_bandwidth_logging(
        self,
    ) -> (
        SwarmBuilder<
            Provider,
            BandwidthMetricsPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
        >,
        Arc<crate::bandwidth::BandwidthSinks>,
    ) {
        #[allow(deprecated)]
        self.without_quic()
            .without_any_other_transports()
            .without_dns()
            .without_websocket()
            .without_relay()
            .with_bandwidth_logging()
    }
}
#[cfg(feature = "metrics")]
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, QuicPhase<T>> {
    pub fn with_bandwidth_metrics(
        self,
        registry: &mut libp2p_metrics::Registry,
    ) -> SwarmBuilder<
        Provider,
        BehaviourPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
    > {
        self.without_quic()
            .without_any_other_transports()
            .without_dns()
            .without_websocket()
            .without_relay()
            .without_bandwidth_logging()
            .with_bandwidth_metrics(registry)
    }
}
