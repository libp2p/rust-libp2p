use std::marker::PhantomData;

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_core::muxing::{StreamMuxer, StreamMuxerBox};
use libp2p_core::upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade};
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_core::Transport;
#[cfg(any(
    all(not(target_arch = "wasm32"), feature = "websocket"),
    feature = "relay"
))]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, UpgradeInfo};
#[cfg(any(
    all(not(target_arch = "wasm32"), feature = "websocket"),
    feature = "relay"
))]
use libp2p_identity::PeerId;

use super::*;
use crate::SwarmBuilder;

pub struct WebsocketPhase<T> {
    pub(crate) transport: T,
}

macro_rules! impl_websocket_builder {
    ($providerKebabCase:literal, $providerPascalCase:ty, $dnsTcp:expr, $websocketStream:ty) => {
        /// Adds a websocket client transport.
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
        ///     .with_websocket(
        ///         (libp2p_tls::Config::new, libp2p_noise::Config::new),
        ///         libp2p_yamux::Config::default,
        ///     )
        ///     .await?
        /// # ;
        /// # Ok(())
        /// # }
        /// ```
        #[cfg(all(not(target_arch = "wasm32"), feature = $providerKebabCase, feature = "websocket"))]
        impl<T> SwarmBuilder<$providerPascalCase, WebsocketPhase<T>> {
            pub async fn with_websocket<
                SecUpgrade,
                SecStream,
                SecError,
                MuxUpgrade,
                MuxStream,
                MuxError,
            >(
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
                T: AuthenticatedMultiplexedTransport,

                SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
                SecError: std::error::Error + Send + Sync + 'static,
                SecUpgrade: IntoSecurityUpgrade<$websocketStream>,
                SecUpgrade::Upgrade: InboundConnectionUpgrade<Negotiated<$websocketStream>, Output = (PeerId, SecStream), Error = SecError> + OutboundConnectionUpgrade<Negotiated<$websocketStream>, Output = (PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
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
                let security_upgrade = security_upgrade.into_security_upgrade(&self.keypair)
                    .map_err(WebsocketErrorInner::SecurityUpgrade)?;
                let websocket_transport = libp2p_websocket::WsConfig::new(
                    $dnsTcp.await.map_err(WebsocketErrorInner::Dns)?,
                )
                    .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                    .authenticate(security_upgrade)
                    .multiplex(multiplexer_upgrade.into_multiplexer_upgrade())
                    .map(|(p, c), _| (p, StreamMuxerBox::new(c)));

                Ok(SwarmBuilder {
                    keypair: self.keypair,
                    phantom: PhantomData,
                    phase: RelayPhase {
                        transport: websocket_transport
                            .or_transport(self.phase.transport)
                            .map(|either, _| either.into_inner()),
                    },
                })
            }
        }
    };
}

impl_websocket_builder!(
    "async-std",
    super::provider::AsyncStd,
    libp2p_dns::async_std::Transport::system(libp2p_tcp::async_io::Transport::new(
        libp2p_tcp::Config::default(),
    )),
    rw_stream_sink::RwStreamSink<
        libp2p_websocket::BytesConnection<libp2p_tcp::async_io::TcpStream>,
    >
);
impl_websocket_builder!(
    "tokio",
    super::provider::Tokio,
    // Note this is an unnecessary await for Tokio Websocket (i.e. tokio dns) in order to be
    // consistent with above AsyncStd construction.
    futures::future::ready(libp2p_dns::tokio::Transport::system(
        libp2p_tcp::tokio::Transport::new(libp2p_tcp::Config::default())
    )),
    rw_stream_sink::RwStreamSink<libp2p_websocket::BytesConnection<libp2p_tcp::tokio::TcpStream>>
);

impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, WebsocketPhase<T>> {
    pub(crate) fn without_websocket(self) -> SwarmBuilder<Provider, RelayPhase<T>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: RelayPhase {
                transport: self.phase.transport,
            },
        }
    }
}

// Shortcuts
#[cfg(feature = "relay")]
impl<T: AuthenticatedMultiplexedTransport, Provider> SwarmBuilder<Provider, WebsocketPhase<T>> {
    /// See [`SwarmBuilder::with_relay_client`].
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
        SecUpgrade::Upgrade: InboundConnectionUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (PeerId, SecStream), Error = SecError> + OutboundConnectionUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
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
        self.without_websocket()
            .with_relay_client(security_upgrade, multiplexer_upgrade)
    }
}
#[cfg(feature = "metrics")]
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, WebsocketPhase<T>> {
    pub fn with_bandwidth_metrics(
        self,
        registry: &mut libp2p_metrics::Registry,
    ) -> SwarmBuilder<
        Provider,
        BehaviourPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
    > {
        self.without_websocket()
            .without_relay()
            .without_bandwidth_logging()
            .with_bandwidth_metrics(registry)
    }
}
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, WebsocketPhase<T>> {
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_websocket()
            .without_relay()
            .without_bandwidth_logging()
            .with_behaviour(constructor)
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
pub struct WebsocketError<Sec>(#[from] WebsocketErrorInner<Sec>);

#[derive(Debug, thiserror::Error)]
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
enum WebsocketErrorInner<Sec> {
    #[error("SecurityUpgrade")]
    SecurityUpgrade(Sec),
    #[cfg(feature = "dns")]
    #[error("Dns")]
    Dns(#[from] std::io::Error),
}
