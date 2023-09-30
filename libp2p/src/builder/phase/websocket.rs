use super::*;
use crate::SwarmBuilder;
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_core::muxing::{StreamMuxer, StreamMuxerBox};
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_core::Transport;
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, UpgradeInfo};
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
use libp2p_identity::PeerId;
use std::marker::PhantomData;

pub struct WebsocketPhase<T, R> {
    pub(crate) transport: T,
    pub(crate) relay_behaviour: R,
}

macro_rules! impl_websocket_builder {
    ($providerKebabCase:literal, $providerCamelCase:ty, $dnsTcp:expr, $websocketStream:ty) => {
        #[cfg(all(not(target_arch = "wasm32"), feature = $providerKebabCase, feature = "websocket"))]
        impl<T, R> SwarmBuilder<$providerCamelCase, WebsocketPhase<T, R>> {
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
                    $providerCamelCase,
                    BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, R>,
                >,
                WebsocketError<SecUpgrade::Error>,
            >

            where
                T: AuthenticatedMultiplexedTransport,

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
                let security_upgrade = security_upgrade.into_security_upgrade(&self.keypair)
                    .map_err(|e| WebsocketError(WebsocketErrorInner::SecurityUpgrade(e)))?;
                let websocket_transport = libp2p_websocket::WsConfig::new(
                    $dnsTcp.await.map_err(|e| WebsocketError(e.into()))?,
                )
                    .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                    .authenticate(security_upgrade)
                    .multiplex(multiplexer_upgrade.into_multiplexer_upgrade())
                    .map(|(p, c), _| (p, StreamMuxerBox::new(c)));

                Ok(SwarmBuilder {
                    keypair: self.keypair,
                    phantom: PhantomData,
                    phase: BandwidthLoggingPhase {
                        transport: websocket_transport
                            .or_transport(self.phase.transport)
                            .map(|either, _| either.into_inner()),
                        relay_behaviour: self.phase.relay_behaviour,
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
// TODO: Unnecessary await for Tokio Websocket (i.e. tokio dns). Not ideal but don't know a better way.
impl_websocket_builder!(
    "tokio",
    super::provider::Tokio,
    futures::future::ready(libp2p_dns::tokio::Transport::system(
        libp2p_tcp::tokio::Transport::new(libp2p_tcp::Config::default())
    )),
    rw_stream_sink::RwStreamSink<libp2p_websocket::BytesConnection<libp2p_tcp::tokio::TcpStream>>
);

impl<Provider, T: AuthenticatedMultiplexedTransport, R>
    SwarmBuilder<Provider, WebsocketPhase<T, R>>
{
    pub(crate) fn without_websocket(self) -> SwarmBuilder<Provider, BandwidthLoggingPhase<T, R>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: BandwidthLoggingPhase {
                relay_behaviour: self.phase.relay_behaviour,
                transport: self.phase.transport,
            },
        }
    }
}

// Shortcuts
#[cfg(feature = "relay")]
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, WebsocketPhase<T, libp2p_relay::client::Behaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_websocket()
            .without_bandwidth_logging()
            .with_behaviour(constructor)
    }
}

impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, WebsocketPhase<T, NoRelayBehaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_websocket()
            .without_bandwidth_logging()
            .with_behaviour(constructor)
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
pub struct WebsocketError<Sec>(WebsocketErrorInner<Sec>);

#[derive(Debug, thiserror::Error)]
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
enum WebsocketErrorInner<Sec> {
    #[error("SecurityUpgrade")]
    SecurityUpgrade(Sec),
    #[cfg(feature = "dns")]
    #[error("Dns")]
    Dns(#[from] std::io::Error),
}
