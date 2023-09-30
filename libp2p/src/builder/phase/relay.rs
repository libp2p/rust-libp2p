use std::marker::PhantomData;

#[cfg(feature = "relay")]
use libp2p_core::muxing::StreamMuxerBox;
#[cfg(feature = "relay")]
use libp2p_core::Transport;
#[cfg(feature = "relay")]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, StreamMuxer, UpgradeInfo};
#[cfg(feature = "relay")]
use libp2p_identity::PeerId;

use crate::SwarmBuilder;

use super::*;

pub struct RelayPhase<T> {
    pub(crate) transport: T,
}

#[cfg(feature = "relay")]
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, RelayPhase<T>> {
    // TODO: This should be with_relay_client.
    pub fn with_relay<SecUpgrade, SecStream, SecError, MuxUpgrade, MuxStream, MuxError>(
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
        let (relay_transport, relay_behaviour) =
            libp2p_relay::client::new(self.keypair.public().to_peer_id());

        Ok(SwarmBuilder {
            phase: WebsocketPhase {
                relay_behaviour,
                transport: self
                    .phase
                    .transport
                    .or_transport(
                        relay_transport
                            .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                            .authenticate(security_upgrade.into_security_upgrade(&self.keypair)?)
                            .multiplex(multiplexer_upgrade.into_multiplexer_upgrade())
                            .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
                    )
                    .map(|either, _| either.into_inner()),
            },
            keypair: self.keypair,
            phantom: PhantomData,
        })
    }
}

pub struct NoRelayBehaviour;

impl<Provider, T> SwarmBuilder<Provider, RelayPhase<T>> {
    pub(crate) fn without_relay(
        self,
    ) -> SwarmBuilder<Provider, WebsocketPhase<T, NoRelayBehaviour>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketPhase {
                transport: self.phase.transport,
                relay_behaviour: NoRelayBehaviour,
            },
        }
    }
}

// Shortcuts
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, RelayPhase<T>> {
    // TODO
    // #[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
    // pub fn with_websocket(
    //     self,
    // ) -> SwarmBuilder<
    //     Provider,
    //     WebsocketTlsPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
    // > {
    //     self.without_relay().with_websocket()
    // }

    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_relay()
            .without_websocket()
            .with_behaviour(constructor)
    }
}
