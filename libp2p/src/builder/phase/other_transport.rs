use std::convert::Infallible;
use std::marker::PhantomData;
use std::sync::Arc;

use libp2p_core::Transport;
#[cfg(feature = "relay")]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, StreamMuxer, UpgradeInfo};
#[cfg(feature = "relay")]
use libp2p_identity::PeerId;

use crate::bandwidth::BandwidthSinks;
use crate::SwarmBuilder;

use super::*;

pub struct OtherTransportPhase<T> {
    pub(crate) transport: T,
}

impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, OtherTransportPhase<T>>
{
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
        Ok(SwarmBuilder {
            phase: OtherTransportPhase {
                transport: self
                    .phase
                    .transport
                    .or_transport(constructor(&self.keypair).try_into_transport()?)
                    .map(|either, _| either.into_inner()),
            },
            keypair: self.keypair,
            phantom: PhantomData,
        })
    }

    pub(crate) fn without_any_other_transports(self) -> SwarmBuilder<Provider, DnsPhase<T>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: DnsPhase {
                transport: self.phase.transport,
            },
        }
    }
}

// Shortcuts
#[cfg(all(not(target_arch = "wasm32"), feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<super::provider::AsyncStd, OtherTransportPhase<T>>
{
    pub async fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<super::provider::AsyncStd, RelayPhase<impl AuthenticatedMultiplexedTransport>>,
        std::io::Error,
    > {
        self.without_any_other_transports().with_dns().await
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<super::provider::Tokio, OtherTransportPhase<T>>
{
    pub fn with_dns(
        self,
    ) -> Result<
        SwarmBuilder<super::provider::Tokio, RelayPhase<impl AuthenticatedMultiplexedTransport>>,
        std::io::Error,
    > {
        self.without_any_other_transports().with_dns()
    }
}
#[cfg(feature = "relay")]
impl<T: AuthenticatedMultiplexedTransport, Provider>
    SwarmBuilder<Provider, OtherTransportPhase<T>>
{
    pub fn with_relay_client<SecUpgrade, SecStream, SecError, MuxUpgrade, MuxStream, MuxError>(
        self,
        security_upgrade: SecUpgrade,
        multiplexer_upgrade: MuxUpgrade,
    ) -> Result<
        SwarmBuilder<
            Provider,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
        >,
        SecUpgrade::Error,
        > where

        SecStream: futures::AsyncRead + futures::AsyncWrite + Unpin + Send + 'static,
        SecError: std::error::Error + Send + Sync + 'static,
        SecUpgrade: IntoSecurityUpgrade<libp2p_relay::client::Connection>,
        SecUpgrade::Upgrade: InboundUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (PeerId, SecStream), Error = SecError> + OutboundUpgrade<Negotiated<libp2p_relay::client::Connection>, Output = (PeerId, SecStream), Error = SecError> + Clone + Send + 'static,
    <SecUpgrade::Upgrade as InboundUpgrade<Negotiated<libp2p_relay::client::Connection>>>::Future: Send,
    <SecUpgrade::Upgrade as OutboundUpgrade<Negotiated<libp2p_relay::client::Connection>>>::Future: Send,
    <<<SecUpgrade as IntoSecurityUpgrade<libp2p_relay::client::Connection>>::Upgrade as UpgradeInfo>::InfoIter as IntoIterator>::IntoIter: Send,
    <<SecUpgrade as IntoSecurityUpgrade<libp2p_relay::client::Connection>>::Upgrade as UpgradeInfo>::Info: Send,

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
        self.without_any_other_transports()
            .without_dns()
            .with_relay_client(security_upgrade, multiplexer_upgrade)
    }
}
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, OtherTransportPhase<T>>
{
    pub fn with_bandwidth_logging(
        self,
    ) -> (
        SwarmBuilder<
            Provider,
            BehaviourPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
        >,
        Arc<BandwidthSinks>,
    ) {
        self.without_any_other_transports()
            .without_dns()
            .without_relay()
            .without_websocket()
            .with_bandwidth_logging()
    }
}
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, OtherTransportPhase<T>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_any_other_transports()
            .without_dns()
            .without_relay()
            .without_websocket()
            .without_bandwidth_logging()
            .with_behaviour(constructor)
    }
}

pub trait TryIntoTransport<T>: private::Sealed<Self::Error> {
    type Error;

    fn try_into_transport(self) -> Result<T, Self::Error>;
}

impl<T> TryIntoTransport<T> for T
where
    T: AuthenticatedMultiplexedTransport,
{
    type Error = Infallible;

    fn try_into_transport(self) -> Result<T, Self::Error> {
        Ok(self)
    }
}

impl<T> TryIntoTransport<T> for Result<T, Box<dyn std::error::Error + Send + Sync>>
where
    T: AuthenticatedMultiplexedTransport,
{
    type Error = TransportError;

    fn try_into_transport(self) -> Result<T, Self::Error> {
        self.map_err(TransportError)
    }
}

mod private {
    pub trait Sealed<Error> {}
}

impl<T: AuthenticatedMultiplexedTransport> private::Sealed<Infallible> for T {}

impl<T: AuthenticatedMultiplexedTransport> private::Sealed<TransportError>
    for Result<T, Box<dyn std::error::Error + Send + Sync>>
{
}

#[derive(Debug, thiserror::Error)]
#[error("failed to build transport: {0}")]
pub struct TransportError(Box<dyn std::error::Error + Send + Sync + 'static>);
