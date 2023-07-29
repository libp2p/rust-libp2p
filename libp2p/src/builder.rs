use libp2p_core::{muxing::StreamMuxerBox, Transport};
use std::marker::PhantomData;

pub struct SwarmBuilder {}

impl SwarmBuilder {
    pub fn new() -> SwarmBuilder {
        Self {}
    }

    pub fn with_new_identity(self) -> ProviderBuilder {
        self.with_existing_identity(libp2p_identity::Keypair::generate_ed25519())
    }

    pub fn with_existing_identity(self, keypair: libp2p_identity::Keypair) -> ProviderBuilder {
        ProviderBuilder { keypair }
    }
}

pub struct ProviderBuilder {
    keypair: libp2p_identity::Keypair,
}

impl ProviderBuilder {
    #[cfg(feature = "async-std")]
    pub fn with_async_std(self) -> TcpBuilder<AsyncStd> {
        TcpBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    #[cfg(feature = "tokio")]
    pub fn with_tokio(self) -> TcpBuilder<Tokio> {
        TcpBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub struct TcpBuilder<P> {
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(all(feature = "async-std", feature = "tcp"))]
impl TcpBuilder<AsyncStd> {
    pub fn with_tcp(self) -> RelayBuilder<AsyncStd, impl AuthenticatedMultiplexedTransport> {
        RelayBuilder {
            transport: libp2p_tcp::async_io::Transport::new(Default::default())
                .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                .authenticate(libp2p_noise::Config::new(&self.keypair).unwrap())
                .multiplex(libp2p_yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

#[cfg(all(feature = "tokio", feature = "tcp"))]
impl TcpBuilder<Tokio> {
    pub fn with_tcp(self) -> RelayBuilder<Tokio, impl AuthenticatedMultiplexedTransport> {
        RelayBuilder {
            transport: libp2p_tcp::tokio::Transport::new(Default::default())
                .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                .authenticate(libp2p_noise::Config::new(&self.keypair).unwrap())
                .multiplex(libp2p_yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

// TODO: without_tcp

pub struct RelayBuilder<P, T> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(feature = "relay")]
impl<P, T: AuthenticatedMultiplexedTransport> RelayBuilder<P, T> {
    // TODO: This should be with_relay_client.
    pub fn with_relay(
        self,
    ) -> OtherTransportBuilder<
        P,
        impl AuthenticatedMultiplexedTransport,
        libp2p_relay::client::Behaviour,
    > {
        let (relay_transport, relay_behaviour) =
            libp2p_relay::client::new(self.keypair.public().to_peer_id());

        OtherTransportBuilder {
            transport: self
                .transport
                .or_transport(
                    relay_transport
                        .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                        .authenticate(libp2p_noise::Config::new(&self.keypair).unwrap())
                        .multiplex(libp2p_yamux::Config::default())
                        .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
                )
                .map(|either, _| either.into_inner()),
            keypair: self.keypair,
            relay_behaviour,
            phantom: PhantomData,
        }
    }
}

pub struct NoRelayBehaviour;

impl<P, T> RelayBuilder<P, T> {
    pub fn without_relay(self) -> OtherTransportBuilder<P, T, NoRelayBehaviour> {
        OtherTransportBuilder {
            transport: self.transport,
            relay_behaviour: NoRelayBehaviour,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub struct OtherTransportBuilder<P, T, R> {
    transport: T,
    relay_behaviour: R,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

impl<P, T: AuthenticatedMultiplexedTransport, R> OtherTransportBuilder<P, T, R> {
    pub fn with_other_transport<OtherTransport: AuthenticatedMultiplexedTransport>(
        self,
        mut constructor: impl FnMut(&libp2p_identity::Keypair) -> OtherTransport,
    ) -> OtherTransportBuilder<P, impl AuthenticatedMultiplexedTransport, R> {
        OtherTransportBuilder {
            transport: self
                .transport
                .or_transport(constructor(&self.keypair))
                .map(|either, _| either.into_inner()),
            relay_behaviour: self.relay_behaviour,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    // TODO: Not the ideal name.
    pub fn no_more_other_transports(
        self,
    ) -> DnsBuilder<P, impl AuthenticatedMultiplexedTransport, R> {
        DnsBuilder {
            transport: self.transport,
            relay_behaviour: self.relay_behaviour,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub struct DnsBuilder<P, T, R> {
    transport: T,
    relay_behaviour: R,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(all(feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport, R> DnsBuilder<AsyncStd, T, R> {
    pub async fn with_dns(self) -> BehaviourBuilder<AsyncStd, R> {
        BehaviourBuilder {
            keypair: self.keypair,
            relay_behaviour: self.relay_behaviour,
            // TODO: Timeout needed?
            transport: libp2p_dns::DnsConfig::system(self.transport)
                .await
                .expect("TODO: Handle")
                .boxed(),
            phantom: PhantomData,
        }
    }
}

#[cfg(all(feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport, R> DnsBuilder<Tokio, T, R> {
    pub fn with_dns(self) -> BehaviourBuilder<Tokio, R> {
        BehaviourBuilder {
            keypair: self.keypair,
            relay_behaviour: self.relay_behaviour,
            // TODO: Timeout needed?
            transport: libp2p_dns::TokioDnsConfig::system(self.transport)
                .expect("TODO: Handle")
                .boxed(),
            phantom: PhantomData,
        }
    }
}

impl<P, T: AuthenticatedMultiplexedTransport, R> DnsBuilder<P, T, R> {
    pub fn without_dns(self) -> BehaviourBuilder<P, R> {
        BehaviourBuilder {
            keypair: self.keypair,
            relay_behaviour: self.relay_behaviour,
            // TODO: Timeout needed?
            transport: self.transport.boxed(),
            phantom: PhantomData,
        }
    }
}

pub struct BehaviourBuilder<P, R> {
    keypair: libp2p_identity::Keypair,
    relay_behaviour: R,
    transport: libp2p_core::transport::Boxed<(libp2p_identity::PeerId, StreamMuxerBox)>,
    phantom: PhantomData<P>,
}

#[cfg(feature = "relay")]
impl<P> BehaviourBuilder<P, libp2p_relay::client::Behaviour> {
    pub fn with_behaviour<B>(
        self,
        mut constructor: impl FnMut(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> B,
    ) -> Builder<P, B> {
        Builder {
            behaviour: constructor(&self.keypair, self.relay_behaviour),
            keypair: self.keypair,
            transport: self.transport,
            phantom: PhantomData,
        }
    }
}

impl<P> BehaviourBuilder<P, NoRelayBehaviour> {
    pub fn with_behaviour<B>(
        self,
        mut constructor: impl FnMut(&libp2p_identity::Keypair) -> B,
    ) -> Builder<P, B> {
        Builder {
            behaviour: constructor(&self.keypair),
            keypair: self.keypair,
            transport: self.transport,
            phantom: PhantomData,
        }
    }
}

pub struct Builder<P, B> {
    keypair: libp2p_identity::Keypair,
    behaviour: B,
    transport: libp2p_core::transport::Boxed<(libp2p_identity::PeerId, StreamMuxerBox)>,
    phantom: PhantomData<P>,
}

#[cfg(feature = "async-std")]
impl<B: libp2p_swarm::NetworkBehaviour> Builder<AsyncStd, B> {
    // TODO: The close should provide the relay transport in case the user used with_relay.
    pub fn build(self) -> libp2p_swarm::Swarm<B> {
        // TODO: Generic over the runtime!
        libp2p_swarm::SwarmBuilder::with_async_std_executor(
            self.transport,
            self.behaviour,
            self.keypair.public().to_peer_id(),
        )
        .build()
    }
}

#[cfg(feature = "tokio")]
impl<B: libp2p_swarm::NetworkBehaviour> Builder<Tokio, B> {
    // TODO: The close should provide the relay transport in case the user used with_relay.
    pub fn build(self) -> libp2p_swarm::Swarm<B> {
        // TODO: Generic over the runtime!
        libp2p_swarm::SwarmBuilder::with_tokio_executor(
            self.transport,
            self.behaviour,
            self.keypair.public().to_peer_id(),
        )
        .build()
    }
}

#[cfg(feature = "async-std")]
pub enum AsyncStd {}

#[cfg(feature = "tokio")]
pub enum Tokio {}

pub trait AuthenticatedMultiplexedTransport:
    Transport<
        Error = Self::E,
        Dial = Self::D,
        ListenerUpgrade = Self::U,
        Output = (libp2p_identity::PeerId, StreamMuxerBox),
    > + Send
    + Unpin
    + 'static
{
    type E: Send + Sync + 'static;
    type D: Send;
    type U: Send;
}

impl<T> AuthenticatedMultiplexedTransport for T
where
    T: Transport<Output = (libp2p_identity::PeerId, StreamMuxerBox)> + Send + Unpin + 'static,
    <T as Transport>::Error: Send + Sync + 'static,
    <T as Transport>::Dial: Send,
    <T as Transport>::ListenerUpgrade: Send,
{
    type E = T::Error;
    type D = T::Dial;
    type U = T::ListenerUpgrade;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp"))]
    fn tcp() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .without_relay()
            .no_more_other_transports()
            .without_dns()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "relay"))]
    fn tcp_relay() {
        #[derive(libp2p_swarm::NetworkBehaviour)]
        #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
        struct Behaviour {
            dummy: libp2p_swarm::dummy::Behaviour,
            relay: libp2p_relay::client::Behaviour,
        }

        let _: libp2p_swarm::Swarm<Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .with_relay()
            .no_more_other_transports()
            .without_dns()
            .with_behaviour(|_, relay| Behaviour {
                dummy: libp2p_swarm::dummy::Behaviour,
                relay,
            })
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "dns"))]
    fn tcp_dns() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .without_relay()
            .no_more_other_transports()
            .with_dns()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .build();
    }

    /// Showcases how to provide custom transports unknown to the libp2p crate, e.g. QUIC or WebRTC.
    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp"))]
    fn tcp_other_transport_other_transport() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .without_relay()
            .with_other_transport(|_| libp2p_core::transport::dummy::DummyTransport::new())
            .with_other_transport(|_| libp2p_core::transport::dummy::DummyTransport::new())
            .with_other_transport(|_| libp2p_core::transport::dummy::DummyTransport::new())
            .no_more_other_transports()
            .without_dns()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .build();
    }
}
