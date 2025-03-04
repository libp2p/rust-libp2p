use std::marker::PhantomData;

#[cfg(all(
    not(target_arch = "wasm32"),
    any(feature = "tcp", feature = "websocket")
))]
use libp2p_core::muxing::{StreamMuxer, StreamMuxerBox};
#[cfg(all(feature = "websocket", not(target_arch = "wasm32")))]
use libp2p_core::Transport;
#[cfg(all(
    not(target_arch = "wasm32"),
    any(feature = "tcp", feature = "websocket")
))]
use libp2p_core::{
    upgrade::InboundConnectionUpgrade, upgrade::OutboundConnectionUpgrade, Negotiated, UpgradeInfo,
};

use super::*;
use crate::SwarmBuilder;

pub struct TcpPhase {}

macro_rules! impl_tcp_builder {
    ($providerKebabCase:literal, $providerPascalCase:ty, $path:ident) => {
        #[cfg(all(
            not(target_arch = "wasm32"),
            feature = "tcp",
            feature = $providerKebabCase,
        ))]
        impl SwarmBuilder<$providerPascalCase, TcpPhase> {
            /// Adds a TCP based transport.
            ///
            /// Note that both `security_upgrade` and `multiplexer_upgrade` take function pointers,
            /// i.e. they take the function themselves (without the invocation via `()`), not the
            /// result of the function invocation. See example below.
            ///
            /// ``` rust
            /// # use libp2p::SwarmBuilder;
            /// # use std::error::Error;
            /// # async fn build_swarm() -> Result<(), Box<dyn Error>> {
            /// let swarm = SwarmBuilder::with_new_identity()
            ///     .with_tokio()
            ///     .with_tcp(
            ///         Default::default(),
            ///         (libp2p_tls::Config::new, libp2p_noise::Config::new),
            ///         libp2p_yamux::Config::default,
            ///     )?
            /// # ;
            /// # Ok(())
            /// # }
            /// ```
            pub fn with_tcp<SecUpgrade, SecStream, SecError, MuxUpgrade, MuxStream, MuxError>(
                self,
                tcp_config: libp2p_tcp::Config,
                security_upgrade: SecUpgrade,
                multiplexer_upgrade: MuxUpgrade,
            ) -> Result<
                SwarmBuilder<$providerPascalCase, QuicPhase<impl AuthenticatedMultiplexedTransport>>,
            SecUpgrade::Error,
            >
            where
                SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
                SecError: std::error::Error + Send + Sync + 'static,
                SecUpgrade: IntoSecurityUpgrade<libp2p_tcp::$path::TcpStream>,
                SecUpgrade::Upgrade: InboundConnectionUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + OutboundConnectionUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>, Output = (libp2p_identity::PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
                <SecUpgrade::Upgrade as InboundConnectionUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>>>::Future: Send,
                <SecUpgrade::Upgrade as OutboundConnectionUpgrade<Negotiated<libp2p_tcp::$path::TcpStream>>>::Future: Send,
                <<<SecUpgrade as IntoSecurityUpgrade<libp2p_tcp::$path::TcpStream>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
                <<SecUpgrade as IntoSecurityUpgrade<libp2p_tcp::$path::TcpStream>>::Upgrade as UpgradeInfo>::Info: Send,

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
    pub(crate) fn without_tcp(
        self,
    ) -> SwarmBuilder<Provider, QuicPhase<impl AuthenticatedMultiplexedTransport>> {
        SwarmBuilder {
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
    ) -> SwarmBuilder<
        super::provider::AsyncStd,
        OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
    > {
        self.without_tcp().with_quic()
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = "tokio"))]
impl SwarmBuilder<super::provider::Tokio, TcpPhase> {
    pub fn with_quic(
        self,
    ) -> SwarmBuilder<
        super::provider::Tokio,
        OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
    > {
        self.without_tcp().with_quic()
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = "async-std"))]
impl SwarmBuilder<super::provider::AsyncStd, TcpPhase> {
    pub fn with_quic_config(
        self,
        constructor: impl FnOnce(libp2p_quic::Config) -> libp2p_quic::Config,
    ) -> SwarmBuilder<
        super::provider::AsyncStd,
        OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
    > {
        self.without_tcp().with_quic_config(constructor)
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = "tokio"))]
impl SwarmBuilder<super::provider::Tokio, TcpPhase> {
    pub fn with_quic_config(
        self,
        constructor: impl FnOnce(libp2p_quic::Config) -> libp2p_quic::Config,
    ) -> SwarmBuilder<
        super::provider::Tokio,
        OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
    > {
        self.without_tcp().with_quic_config(constructor)
    }
}
impl<Provider> SwarmBuilder<Provider, TcpPhase> {
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
        self.without_tcp()
            .without_quic()
            .with_other_transport(constructor)
    }
}
macro_rules! impl_tcp_phase_with_websocket {
    ($providerKebabCase:literal, $providerPascalCase:ty, $websocketStream:ty) => {
        #[cfg(all(feature = $providerKebabCase, not(target_arch = "wasm32"), feature = "websocket"))]
        impl SwarmBuilder<$providerPascalCase, TcpPhase> {
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
                    WebsocketError<SecUpgrade::Error>,
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
                self.without_tcp()
                    .without_quic()
                    .without_any_other_transports()
                    .without_dns()
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
