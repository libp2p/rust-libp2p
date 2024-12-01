use std::marker::PhantomData;

#[cfg(feature = "relay")]
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade};
#[cfg(feature = "relay")]
use libp2p_core::Transport;
#[cfg(any(feature = "relay", feature = "websocket"))]
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, StreamMuxer, UpgradeInfo};
#[cfg(feature = "relay")]
use libp2p_identity::PeerId;

use super::*;
use crate::SwarmBuilder;

pub struct RelayPhase<T> {
    pub(crate) transport: T,
}

#[cfg(feature = "relay")]
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, RelayPhase<T>> {
    /// Adds a relay client transport.
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
    ///      .with_relay_client(
    ///          (libp2p_tls::Config::new, libp2p_noise::Config::new),
    ///          libp2p_yamux::Config::default,
    ///      )?
    /// # ;
    /// # Ok(())
    /// # }
    /// ```
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
        let (relay_transport, relay_behaviour) =
            libp2p_relay::client::new(self.keypair.public().to_peer_id());
        let relay_transport = relay_transport
            .upgrade(libp2p_core::upgrade::Version::V1Lazy)
            .authenticate(security_upgrade.into_security_upgrade(&self.keypair)?)
            .multiplex(multiplexer_upgrade.into_multiplexer_upgrade())
            .map(|(p, c), _| (p, StreamMuxerBox::new(c)));

        Ok(SwarmBuilder {
            phase: BandwidthLoggingPhase {
                relay_behaviour,
                transport: relay_transport
                    .or_transport(self.phase.transport)
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
    ) -> SwarmBuilder<Provider, BandwidthLoggingPhase<T, NoRelayBehaviour>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: BandwidthLoggingPhase {
                transport: self.phase.transport,
                relay_behaviour: NoRelayBehaviour,
            },
        }
    }
}

// Shortcuts
#[cfg(feature = "metrics")]
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, RelayPhase<T>> {
    pub fn with_bandwidth_metrics(
        self,
        registry: &mut libp2p_metrics::Registry,
    ) -> SwarmBuilder<
        Provider,
        BehaviourPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
    > {
        self.without_relay()
            .without_bandwidth_logging()
            .with_bandwidth_metrics(registry)
    }
}
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, RelayPhase<T>> {
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_relay()
            .without_bandwidth_logging()
            .without_bandwidth_metrics()
            .with_behaviour(constructor)
    }
}
