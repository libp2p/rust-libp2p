use super::*;
use crate::SwarmBuilder;
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_core::muxing::StreamMuxer;
#[cfg(any(
    feature = "relay",
    all(not(target_arch = "wasm32"), feature = "websocket")
))]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, UpgradeInfo};
use std::marker::PhantomData;

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
            super::websocket::WebsocketPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
        >,
        SecUpgrade::Error,
        > where

        SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
        SecError: std::error::Error + Send + Sync + 'static,
        SecUpgrade: IntoSecurityUpgrade<libp2p_relay::client::Connection>,
        SecUpgrade::Upgrade: InboundUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + OutboundUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
    <SecUpgrade::Upgrade as InboundUpgrade<Negotiated<libp2p_relay::client::Connection>>>::Future: Send,
    <SecUpgrade::Upgrade as OutboundUpgrade<Negotiated<libp2p_relay::client::Connection>>>::Future: Send,
    <<<SecUpgrade as IntoSecurityUpgrade<libp2p_relay::client::Connection>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
    <<SecUpgrade as IntoSecurityUpgrade<libp2p_relay::client::Connection>>::Upgrade as UpgradeInfo>::Info: Send,

        MuxStream: libp2p_core::muxing::StreamMuxer + Send + 'static,
        MuxStream::Substream: Send + 'static,
        MuxStream::Error: Send + Sync + 'static,
        MuxUpgrade: IntoMultiplexerUpgrade<SecStream>,
        MuxUpgrade::Upgrade: InboundUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + OutboundUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + Clone + Send + 'static,
    <MuxUpgrade::Upgrade as InboundUpgrade<Negotiated<SecStream>>>::Future: Send,
    <MuxUpgrade::Upgrade as OutboundUpgrade<Negotiated<SecStream>>>::Future: Send,
        MuxError: std::error::Error + Send + Sync + 'static,
    <<<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
    <<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::Info: Send,
    {
        self.without_quic()
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
            .without_relay()
            .without_websocket()
            .with_behaviour(constructor)
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::AsyncStd, QuicPhase<T>> {
    pub async fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<super::provider::AsyncStd, RelayPhase<impl AuthenticatedMultiplexedTransport>>,
        std::io::Error,
    > {
        self.without_quic()
            .without_any_other_transports()
            .with_dns()
            .await
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<super::provider::Tokio, QuicPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<super::provider::Tokio, RelayPhase<impl AuthenticatedMultiplexedTransport>>,
        std::io::Error,
    > {
        self.without_quic()
            .without_any_other_transports()
            .with_dns()
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
                        BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
                    >,
                    super::websocket::WebsocketError<SecUpgrade::Error>,
                >
            where
                SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
                SecError: std::error::Error + Send + Sync + 'static,
                SecUpgrade: IntoSecurityUpgrade<$websocketStream>,
                SecUpgrade::Upgrade: InboundUpgrade<Negotiated<$websocketStream>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + OutboundUpgrade<Negotiated<$websocketStream>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
            <SecUpgrade::Upgrade as InboundUpgrade<Negotiated<$websocketStream>>>::Future: Send,
            <SecUpgrade::Upgrade as OutboundUpgrade<Negotiated<$websocketStream>>>::Future: Send,
            <<<SecUpgrade as IntoSecurityUpgrade<$websocketStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
            <<SecUpgrade as IntoSecurityUpgrade<$websocketStream>>::Upgrade as UpgradeInfo>::Info: Send,

                MuxStream: StreamMuxer + Send + 'static,
                MuxStream::Substream: Send + 'static,
                MuxStream::Error: Send + Sync + 'static,
                MuxUpgrade: IntoMultiplexerUpgrade<SecStream>,
                MuxUpgrade::Upgrade: InboundUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + OutboundUpgrade<Negotiated<SecStream>, Output = MuxStream, Error = MuxError> + Clone + Send + 'static,
                <MuxUpgrade::Upgrade as InboundUpgrade<Negotiated<SecStream>>>::Future: Send,
                <MuxUpgrade::Upgrade as OutboundUpgrade<Negotiated<SecStream>>>::Future: Send,
                    MuxError: std::error::Error + Send + Sync + 'static,
                <<<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
                <<MuxUpgrade as IntoMultiplexerUpgrade<SecStream>>::Upgrade as UpgradeInfo>::Info: Send,
            {
                self.without_quic()
                    .without_any_other_transports()
                    .without_dns()
                    .without_relay()
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
