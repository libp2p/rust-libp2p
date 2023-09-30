use super::*;
#[cfg(feature = "websocket")]
use libp2p_core::Transport;
use crate::SwarmBuilder;
#[cfg(any(feature = "tcp", feature = "websocket"))]
use libp2p_core::muxing::{StreamMuxer, StreamMuxerBox};
#[cfg(any(feature = "tcp", feature = "websocket"))]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, UpgradeInfo};
#[cfg(any(feature = "tcp", feature = "websocket"))]
use libp2p_identity::PeerId;
use std::marker::PhantomData;

pub struct TcpPhase {}

macro_rules! impl_tcp_builder {
    ($providerKebabCase:literal, $providerCamelCase:ty, $path:ident) => {
        #[cfg(all(
            not(target_arch = "wasm32"),
            feature = "tcp",
            feature = $providerKebabCase,
        ))]
        impl SwarmBuilder<$providerCamelCase, TcpPhase> {
            pub fn with_tcp<SecUpgrade, SecStream, SecError, MuxUpgrade, MuxStream, MuxError>(
                self,
                tcp_config: libp2p_tcp::Config,
                security_upgrade: SecUpgrade,
                multiplexer_upgrade: MuxUpgrade,
            ) -> Result<
                SwarmBuilder<$providerCamelCase, QuicPhase<impl AuthenticatedMultiplexedTransport>>,
            SecUpgrade::Error,
            >
            where
                SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
                SecError: std::error::Error + Send + Sync + 'static,
                SecUpgrade: IntoSecurityUpgrade<libp2p_tcp::$path::TcpStream>,
                SecUpgrade::Upgrade: InboundUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>, Output = (PeerId, SecStream), Error = SecError> + OutboundUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>, Output = (PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
                <SecUpgrade::Upgrade as InboundUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>>>::Future: Send,
                <SecUpgrade::Upgrade as OutboundUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>>>::Future: Send,
                <<<SecUpgrade as IntoSecurityUpgrade<libp2p_tcp::$path::TcpStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
                <<SecUpgrade as IntoSecurityUpgrade<libp2p_tcp::$path::TcpStream>>::Upgrade as UpgradeInfo>::Info: Send,

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
                Ok(SwarmBuilder {
                    phase: QuicPhase {
                        transport: libp2p_tcp::$path::Transport::new(tcp_config)
                            .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                            .authenticate(
                                security_upgrade.into_security_upgrade(&self.keypair)?,
                            )
                            .multiplex(multiplexer_upgrade.into_multiplexer_upgrade())
                            .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
                    },
                    keypair: self.keypair,
                    phantom: PhantomData,
                })
            }
        }
    };
}

impl_tcp_builder!("async-std", super::provider::AsyncStd, async_io);
impl_tcp_builder!("tokio", super::provider::Tokio, tokio);

impl<Provider> SwarmBuilder<Provider, TcpPhase> {
    // TODO: This would allow one to build a faulty transport.
    pub(crate) fn without_tcp(
        self,
    ) -> SwarmBuilder<Provider, QuicPhase<impl AuthenticatedMultiplexedTransport>> {
        SwarmBuilder {
            // TODO: Is this a good idea in a production environment? Unfortunately I don't know a
            // way around it. One can not define two `with_relay` methods, one with a real transport
            // using OrTransport, one with a fake transport discarding it right away.
            keypair: self.keypair,
            phantom: PhantomData,
            phase: QuicPhase {
                transport: libp2p_core::transport::dummy::DummyTransport::new(),
            },
        }
    }
}

// Shortcuts
#[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = "async-std"))]
impl SwarmBuilder<super::provider::AsyncStd, TcpPhase> {
    pub fn with_quic(
        self,
    ) -> SwarmBuilder<super::provider::AsyncStd, OtherTransportPhase<impl AuthenticatedMultiplexedTransport>> {
        self.without_tcp().with_quic()
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = "tokio"))]
impl SwarmBuilder<super::provider::Tokio, TcpPhase> {
    pub fn with_quic(
        self,
    ) -> SwarmBuilder<super::provider::Tokio, OtherTransportPhase<impl AuthenticatedMultiplexedTransport>> {
        self.without_tcp().with_quic()
    }
}
impl<Provider> SwarmBuilder<Provider, TcpPhase> {
    pub fn with_other_transport<
        OtherTransport: AuthenticatedMultiplexedTransport,
        R: TryIntoTransport<OtherTransport>,
    >(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<
        SwarmBuilder<Provider, OtherTransportPhase<impl AuthenticatedMultiplexedTransport>>,
        R::Error,
    > {
        self.without_tcp()
            .without_quic()
            .with_other_transport(constructor)
    }
}
macro_rules! impl_tcp_phase_with_websocket {
    ($providerKebabCase:literal, $providerCamelCase:ty, $websocketStream:ty) => {
        #[cfg(all(feature = $providerKebabCase, not(target_arch = "wasm32"), feature = "websocket"))]
        impl SwarmBuilder<$providerCamelCase, TcpPhase> {
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
                        $providerCamelCase,
                        BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
                    >,
                    WebsocketError<SecUpgrade::Error>,
                >
            where
                SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
                SecError: std::error::Error + Send + Sync + 'static,
                SecUpgrade: IntoSecurityUpgrade<$websocketStream>,
                SecUpgrade::Upgrade: InboundUpgrade<Negotiated<$websocketStream>, Output = (PeerId, SecStream), Error = SecError> + OutboundUpgrade<Negotiated<$websocketStream>, Output = (PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
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
                self.without_tcp()
                    .without_quic()
                    .without_any_other_transports()
                    .without_dns()
                    .without_relay()
                    .with_websocket(security_upgrade, multiplexer_upgrade)
                    .await
            }
        }
    }
}
impl_tcp_phase_with_websocket!(
    "async-std",
    super::provider::AsyncStd,
    rw_stream_sink::RwStreamSink<
        libp2p_websocket::BytesConnection<libp2p_tcp::async_io::TcpStream>,
    >
);
impl_tcp_phase_with_websocket!(
    "tokio",
    super::provider::Tokio,
    rw_stream_sink::RwStreamSink<libp2p_websocket::BytesConnection<libp2p_tcp::tokio::TcpStream>>
);
