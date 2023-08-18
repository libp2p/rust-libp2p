// TODO: Rename runtime to provider
// TODO: Should we have a timeout on transport?
// TODO: Be able to address `SwarmBuilder` configuration methods.
// TODO: Consider moving with_relay after with_other_transport.
// TODO: Consider making with_other_transport fallible.
// TODO: Add with_quic

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

#[cfg(feature = "tcp")]
impl<P> TcpBuilder<P> {
    pub fn with_tcp(self) -> TcpTlsBuilder<P> {
        self.with_tcp_config(Default::default())
    }

    pub fn with_tcp_config(self, config: libp2p_tcp::Config) -> TcpTlsBuilder<P> {
        TcpTlsBuilder {
            config,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

impl<P> TcpBuilder<P> {
    // TODO: This would allow one to build a faulty transport.
    pub fn without_tcp(self) -> RelayBuilder<P, impl AuthenticatedMultiplexedTransport> {
        RelayBuilder {
            // TODO: Is this a good idea in a production environment? Unfortunately I don't know a
            // way around it. One can not define two `with_relay` methods, one with a real transport
            // using OrTransport, one with a fake transport discarding it right away.
            transport: libp2p_core::transport::dummy::DummyTransport::new(),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

#[cfg(feature = "tcp")]
pub struct TcpTlsBuilder<P> {
    config: libp2p_tcp::Config,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(feature = "tcp")]
impl<P> TcpTlsBuilder<P> {
    #[cfg(feature = "tls")]
    pub fn with_tls(self) -> TcpNoiseBuilder<P, Tls> {
        TcpNoiseBuilder {
            config: self.config,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    pub fn without_tls(self) -> TcpNoiseBuilder<P, WithoutTls> {
        TcpNoiseBuilder {
            config: self.config,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

// Shortcuts
#[cfg(all(feature = "tcp", feature = "noise", feature = "async-std"))]
impl TcpTlsBuilder<AsyncStd> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<QuicBuilder<AsyncStd, impl AuthenticatedMultiplexedTransport>, AuthenticationError>
    {
        self.without_tls().with_noise()
    }
}
#[cfg(all(feature = "tcp", feature = "noise", feature = "tokio"))]
impl TcpTlsBuilder<Tokio> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<QuicBuilder<Tokio, impl AuthenticatedMultiplexedTransport>, AuthenticationError>
    {
        self.without_tls().with_noise()
    }
}

#[cfg(feature = "tcp")]
pub struct TcpNoiseBuilder<P, A> {
    config: libp2p_tcp::Config,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<(P, A)>,
}

#[cfg(feature = "tcp")]
macro_rules! construct_quic_builder {
    ($self:ident, $tcp:ident, $auth:expr) => {
        Ok(QuicBuilder {
            transport: libp2p_tcp::$tcp::Transport::new($self.config)
                .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                .authenticate($auth)
                .multiplex(libp2p_yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
            keypair: $self.keypair,
            phantom: PhantomData,
        })
    };
}

macro_rules! impl_tcp_noise_builder {
    ($runtimeKebabCase:literal, $runtimeCamelCase:ident, $tcp:ident) => {
        #[cfg(all(feature = $runtimeKebabCase, feature = "tcp", feature = "tls"))]
        impl TcpNoiseBuilder<$runtimeCamelCase, Tls> {
            #[cfg(feature = "noise")]
            pub fn with_noise(
                self,
            ) -> Result<
                QuicBuilder<$runtimeCamelCase, impl AuthenticatedMultiplexedTransport>,
                AuthenticationError,
            > {
                construct_quic_builder!(
                    self,
                    $tcp,
                    libp2p_core::upgrade::Map::new(
                        libp2p_core::upgrade::SelectUpgrade::new(
                            libp2p_tls::Config::new(&self.keypair)?,
                            libp2p_noise::Config::new(&self.keypair)?,
                        ),
                        |upgrade| match upgrade {
                            futures::future::Either::Left((peer_id, upgrade)) => {
                                (peer_id, futures::future::Either::Left(upgrade))
                            }
                            futures::future::Either::Right((peer_id, upgrade)) => {
                                (peer_id, futures::future::Either::Right(upgrade))
                            }
                        },
                    )
                )
            }

            pub fn without_noise(
                self,
            ) -> Result<
                QuicBuilder<$runtimeCamelCase, impl AuthenticatedMultiplexedTransport>,
                AuthenticationError,
            > {
                construct_quic_builder!(self, $tcp, libp2p_tls::Config::new(&self.keypair)?)
            }
        }

        #[cfg(feature = $runtimeKebabCase)]
        impl TcpNoiseBuilder<$runtimeCamelCase, WithoutTls> {
            #[cfg(feature = "noise")]
            pub fn with_noise(
                self,
            ) -> Result<
                QuicBuilder<$runtimeCamelCase, impl AuthenticatedMultiplexedTransport>,
                AuthenticationError,
            > {
                construct_quic_builder!(self, $tcp, libp2p_noise::Config::new(&self.keypair)?)
            }
        }
    };
}

impl_tcp_noise_builder!("async-std", AsyncStd, async_io);
impl_tcp_noise_builder!("tokio", Tokio, tokio);

#[cfg(feature = "tls")]
pub enum Tls {}

pub enum WithoutTls {}

#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError {
    #[error("Tls")]
    #[cfg(feature = "tls")]
    Tls(#[from] libp2p_tls::certificate::GenError),
    #[error("Noise")]
    #[cfg(feature = "noise")]
    Noise(#[from] libp2p_noise::Error),
}

pub struct QuicBuilder<P, T> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(all(feature = "quic", feature = "async-std"))]
impl<T> QuicBuilder<AsyncStd, T> {
    pub fn with_quic(self) -> RelayBuilder<AsyncStd, impl AuthenticatedMultiplexedTransport> {
        RelayBuilder {
            transport: self
                .transport
                .or(
                    quic::async_std::Transport::new(quic::Config::new(&self.keypair))
                        .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer))),
                ),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

#[cfg(all(feature = "quic", feature = "tokio"))]
impl<T> QuicBuilder<Tokio, T> {
    pub fn with_quic(self) -> RelayBuilder<Tokio, impl AuthenticatedMultiplexedTransport> {
        RelayBuilder {
            transport: self
                .transport
                .or(
                    quic::tokio::Transport::new(quic::Config::new(&self.keypair))
                        .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer))),
                ),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

impl<P, T> QuicBuilder<P, T> {
    pub fn without_quic(self) -> RelayBuilder<P, T> {
        RelayBuilder {
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

// Shortcuts
impl<P, T: AuthenticatedMultiplexedTransport> QuicBuilder<P, T> {
    pub fn with_relay(self) -> RelayTlsBuilder<P, T> {
        RelayTlsBuilder {
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    pub fn with_other_transport<OtherTransport: AuthenticatedMultiplexedTransport>(
        self,
        constructor: impl FnMut(&libp2p_identity::Keypair) -> OtherTransport,
    ) -> OtherTransportBuilder<P, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour> {
        self.without_quic()
            .without_relay()
            .with_other_transport(constructor)
    }

    #[cfg(feature = "websocket")]
    pub fn with_websocket(
        self,
    ) -> WebsocketTlsBuilder<P, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour> {
        self.without_quic()
            .without_relay()
            .without_any_other_transports()
            .without_dns()
            .with_websocket()
    }

    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(&libp2p_identity::Keypair) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_quic()
            .without_relay()
            .without_any_other_transports()
            .without_dns()
            .without_websocket()
            .with_behaviour(constructor)
    }
}
// Shortcuts
#[cfg(all(feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> QuicBuilder<AsyncStd, T> {
    pub async fn with_dns(
        self,
    ) -> Result<
        WebsocketBuilder<AsyncStd, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
        std::io::Error,
    > {
        self.without_quic()
            .without_relay()
            .without_any_other_transports()
            .with_dns()
            .await
    }
}
#[cfg(all(feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> QuicBuilder<Tokio, T> {
    pub fn with_dns(
        self,
    ) -> Result<
        WebsocketBuilder<Tokio, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
        std::io::Error,
    > {
        self.without_quic()
            .without_relay()
            .without_any_other_transports()
            .with_dns()
    }
}

pub struct RelayBuilder<P, T> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

// TODO: Noise feature
#[cfg(feature = "relay")]
impl<P, T> RelayBuilder<P, T> {
    // TODO: This should be with_relay_client.
    pub fn with_relay(self) -> RelayTlsBuilder<P, T> {
        RelayTlsBuilder {
            transport: self.transport,
            keypair: self.keypair,
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

// Shortcuts
impl<P, T: AuthenticatedMultiplexedTransport> RelayBuilder<P, T> {
    pub fn with_other_transport<OtherTransport: AuthenticatedMultiplexedTransport>(
        self,
        constructor: impl FnMut(&libp2p_identity::Keypair) -> OtherTransport,
    ) -> OtherTransportBuilder<P, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour> {
        self.without_relay().with_other_transport(constructor)
    }

    #[cfg(feature = "websocket")]
    pub fn with_websocket(
        self,
    ) -> WebsocketTlsBuilder<P, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour> {
        self.without_relay()
            .without_any_other_transports()
            .without_dns()
            .with_websocket()
    }

    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(&libp2p_identity::Keypair) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_relay()
            .without_any_other_transports()
            .without_dns()
            .without_websocket()
            .with_behaviour(constructor)
    }
}
// Shortcuts
#[cfg(all(feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> RelayBuilder<AsyncStd, T> {
    pub async fn with_dns(
        self,
    ) -> Result<
        WebsocketBuilder<AsyncStd, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
        std::io::Error,
    > {
        self.without_relay()
            .without_any_other_transports()
            .with_dns()
            .await
    }
}
#[cfg(all(feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> RelayBuilder<Tokio, T> {
    pub fn with_dns(
        self,
    ) -> Result<
        WebsocketBuilder<Tokio, impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
        std::io::Error,
    > {
        self.without_relay()
            .without_any_other_transports()
            .with_dns()
    }
}

#[cfg(feature = "relay")]
pub struct RelayTlsBuilder<P, T> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(feature = "relay")]
impl<P, T> RelayTlsBuilder<P, T> {
    #[cfg(feature = "tls")]
    pub fn with_tls(self) -> RelayNoiseBuilder<P, T, Tls> {
        RelayNoiseBuilder {
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    pub fn without_tls(self) -> RelayNoiseBuilder<P, T, WithoutTls> {
        RelayNoiseBuilder {
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

// Shortcuts
#[cfg(all(feature = "relay", feature = "noise", feature = "async-std"))]
impl<T: AuthenticatedMultiplexedTransport> RelayTlsBuilder<AsyncStd, T> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        OtherTransportBuilder<
            AsyncStd,
            impl AuthenticatedMultiplexedTransport,
            libp2p_relay::client::Behaviour,
        >,
        AuthenticationError,
    > {
        self.without_tls().with_noise()
    }
}
#[cfg(all(feature = "relay", feature = "noise", feature = "tokio"))]
impl<T: AuthenticatedMultiplexedTransport> RelayTlsBuilder<Tokio, T> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        OtherTransportBuilder<
            Tokio,
            impl AuthenticatedMultiplexedTransport,
            libp2p_relay::client::Behaviour,
        >,
        AuthenticationError,
    > {
        self.without_tls().with_noise()
    }
}

#[cfg(feature = "relay")]
pub struct RelayNoiseBuilder<P, T, A> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<(P, A)>,
}

#[cfg(feature = "relay")]
macro_rules! construct_other_transport_builder {
    ($self:ident, $auth:expr) => {{
        let (relay_transport, relay_behaviour) =
            libp2p_relay::client::new($self.keypair.public().to_peer_id());

        Ok(OtherTransportBuilder {
            transport: $self
                .transport
                .or_transport(
                    relay_transport
                        .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                        .authenticate($auth)
                        .multiplex(libp2p_yamux::Config::default())
                        .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
                )
                .map(|either, _| either.into_inner()),
            keypair: $self.keypair,
            relay_behaviour,
            phantom: PhantomData,
        })
    }};
}

#[cfg(all(feature = "relay", feature = "tls"))]
impl<P, T: AuthenticatedMultiplexedTransport> RelayNoiseBuilder<P, T, Tls> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        OtherTransportBuilder<
            P,
            impl AuthenticatedMultiplexedTransport,
            libp2p_relay::client::Behaviour,
        >,
        AuthenticationError,
    > {
        construct_other_transport_builder!(
            self,
            libp2p_core::upgrade::Map::new(
                libp2p_core::upgrade::SelectUpgrade::new(
                    libp2p_tls::Config::new(&self.keypair)?,
                    libp2p_noise::Config::new(&self.keypair)?,
                ),
                |upgrade| match upgrade {
                    futures::future::Either::Left((peer_id, upgrade)) => {
                        (peer_id, futures::future::Either::Left(upgrade))
                    }
                    futures::future::Either::Right((peer_id, upgrade)) => {
                        (peer_id, futures::future::Either::Right(upgrade))
                    }
                },
            )
        )
    }
    pub fn without_noise(
        self,
    ) -> Result<
        OtherTransportBuilder<
            P,
            impl AuthenticatedMultiplexedTransport,
            libp2p_relay::client::Behaviour,
        >,
        AuthenticationError,
    > {
        construct_other_transport_builder!(self, libp2p_tls::Config::new(&self.keypair)?)
    }
}

#[cfg(feature = "relay")]
impl<P, T: AuthenticatedMultiplexedTransport> RelayNoiseBuilder<P, T, WithoutTls> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        OtherTransportBuilder<
            P,
            impl AuthenticatedMultiplexedTransport,
            libp2p_relay::client::Behaviour,
        >,
        AuthenticationError,
    > {
        construct_other_transport_builder!(self, libp2p_noise::Config::new(&self.keypair)?)
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
    pub fn without_any_other_transports(
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

// Shortcuts
#[cfg(all(feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport, R> OtherTransportBuilder<AsyncStd, T, R> {
    pub async fn with_dns(
        self,
    ) -> Result<WebsocketBuilder<AsyncStd, impl AuthenticatedMultiplexedTransport, R>, std::io::Error>
    {
        self.without_any_other_transports().with_dns().await
    }
}
#[cfg(all(feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport, R> OtherTransportBuilder<Tokio, T, R> {
    pub fn with_dns(
        self,
    ) -> Result<WebsocketBuilder<Tokio, impl AuthenticatedMultiplexedTransport, R>, std::io::Error>
    {
        self.without_any_other_transports().with_dns()
    }
}
#[cfg(feature = "relay")]
impl<P, T: AuthenticatedMultiplexedTransport>
    OtherTransportBuilder<P, T, libp2p_relay::client::Behaviour>
{
    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(
            &libp2p_identity::Keypair,
            libp2p_relay::client::Behaviour,
        ) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_any_other_transports()
            .without_dns()
            .without_websocket()
            .with_behaviour(constructor)
    }
}
impl<P, T: AuthenticatedMultiplexedTransport> OtherTransportBuilder<P, T, NoRelayBehaviour> {
    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(&libp2p_identity::Keypair) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_any_other_transports()
            .without_dns()
            .without_websocket()
            .with_behaviour(constructor)
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
    pub async fn with_dns(
        self,
    ) -> Result<WebsocketBuilder<AsyncStd, impl AuthenticatedMultiplexedTransport, R>, std::io::Error>
    {
        Ok(WebsocketBuilder {
            keypair: self.keypair,
            relay_behaviour: self.relay_behaviour,
            transport: libp2p_dns::DnsConfig::system(self.transport).await?,
            phantom: PhantomData,
        })
    }
}

#[cfg(all(feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport, R> DnsBuilder<Tokio, T, R> {
    pub fn with_dns(
        self,
    ) -> Result<WebsocketBuilder<Tokio, impl AuthenticatedMultiplexedTransport, R>, std::io::Error>
    {
        Ok(WebsocketBuilder {
            keypair: self.keypair,
            relay_behaviour: self.relay_behaviour,
            transport: libp2p_dns::TokioDnsConfig::system(self.transport)?,
            phantom: PhantomData,
        })
    }
}

impl<P, T, R> DnsBuilder<P, T, R> {
    pub fn without_dns(self) -> WebsocketBuilder<P, T, R> {
        WebsocketBuilder {
            keypair: self.keypair,
            relay_behaviour: self.relay_behaviour,
            // TODO: Timeout needed?
            transport: self.transport,
            phantom: PhantomData,
        }
    }
}

// Shortcuts
#[cfg(feature = "relay")]
impl<P, T: AuthenticatedMultiplexedTransport> DnsBuilder<P, T, libp2p_relay::client::Behaviour> {
    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(
            &libp2p_identity::Keypair,
            libp2p_relay::client::Behaviour,
        ) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_dns()
            .without_websocket()
            .with_behaviour(constructor)
    }
}
impl<P, T: AuthenticatedMultiplexedTransport> DnsBuilder<P, T, NoRelayBehaviour> {
    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(&libp2p_identity::Keypair) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_dns()
            .without_websocket()
            .with_behaviour(constructor)
    }
}

pub struct WebsocketBuilder<P, T, R> {
    transport: T,
    relay_behaviour: R,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(feature = "websocket")]
impl<P, T, R> WebsocketBuilder<P, T, R> {
    pub fn with_websocket(self) -> WebsocketTlsBuilder<P, T, R> {
        WebsocketTlsBuilder {
            transport: self.transport,
            relay_behaviour: self.relay_behaviour,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

impl<P, T: AuthenticatedMultiplexedTransport, R> WebsocketBuilder<P, T, R> {
    pub fn without_websocket(self) -> BehaviourBuilder<P, R> {
        BehaviourBuilder {
            keypair: self.keypair,
            relay_behaviour: self.relay_behaviour,
            // TODO: Timeout needed?
            transport: self.transport.boxed(),
            phantom: PhantomData,
        }
    }
}

// Shortcuts
#[cfg(feature = "relay")]
impl<P, T: AuthenticatedMultiplexedTransport>
    WebsocketBuilder<P, T, libp2p_relay::client::Behaviour>
{
    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(
            &libp2p_identity::Keypair,
            libp2p_relay::client::Behaviour,
        ) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_websocket().with_behaviour(constructor)
    }
}
impl<P, T: AuthenticatedMultiplexedTransport> WebsocketBuilder<P, T, NoRelayBehaviour> {
    pub fn with_behaviour<B>(
        self,
        constructor: impl FnMut(&libp2p_identity::Keypair) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        self.without_websocket().with_behaviour(constructor)
    }
}

#[cfg(feature = "websocket")]
pub struct WebsocketTlsBuilder<P, T, R> {
    transport: T,
    relay_behaviour: R,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(feature = "websocket")]
impl<P, T, R> WebsocketTlsBuilder<P, T, R> {
    #[cfg(feature = "tls")]
    pub fn with_tls(self) -> WebsocketNoiseBuilder<P, T, R, Tls> {
        WebsocketNoiseBuilder {
            relay_behaviour: self.relay_behaviour,
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    pub fn without_tls(self) -> WebsocketNoiseBuilder<P, T, R, WithoutTls> {
        WebsocketNoiseBuilder {
            relay_behaviour: self.relay_behaviour,
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

#[cfg(feature = "websocket")]
pub struct WebsocketNoiseBuilder<P, T, R, A> {
    transport: T,
    relay_behaviour: R,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<(P, A)>,
}

#[cfg(feature = "websocket")]
macro_rules! construct_behaviour_builder {
    ($self:ident, $dnsTcp:expr, $auth:expr) => {{
        let websocket_transport = libp2p_websocket::WsConfig::new($dnsTcp.await?)
            .upgrade(libp2p_core::upgrade::Version::V1)
            .authenticate($auth)
            .multiplex(libp2p_yamux::Config::default())
            .map(|(p, c), _| (p, StreamMuxerBox::new(c)));

        Ok(BehaviourBuilder {
            transport: websocket_transport
                .or_transport($self.transport)
                .map(|either, _| either.into_inner())
                .boxed(),
            keypair: $self.keypair,
            relay_behaviour: $self.relay_behaviour,
            phantom: PhantomData,
        })
    }};
}

macro_rules! impl_websocket_noise_builder {
    ($runtimeKebabCase:literal, $runtimeCamelCase:ident, $dnsTcp:expr) => {
        #[cfg(all(
                                                                    feature = $runtimeKebabCase,
                                                                    feature = "websocket",
                                                                    feature = "dns",
                                                                    feature = "tls"
                                                                ))]
        impl<T: AuthenticatedMultiplexedTransport, R>
            WebsocketNoiseBuilder<$runtimeCamelCase, T, R, Tls>
        {
            #[cfg(feature = "noise")]
            pub async fn with_noise(self) -> Result<BehaviourBuilder<$runtimeCamelCase, R>,WebsocketError> {
                construct_behaviour_builder!(
                    self,
                    $dnsTcp,
                    libp2p_core::upgrade::Map::new(
                        libp2p_core::upgrade::SelectUpgrade::new(
                            libp2p_tls::Config::new(&self.keypair).map_err(Into::<AuthenticationError>::into)?,
                            libp2p_noise::Config::new(&self.keypair).map_err(Into::<AuthenticationError>::into)?,
                        ),
                        |upgrade| match upgrade {
                            futures::future::Either::Left((peer_id, upgrade)) => {
                                (peer_id, futures::future::Either::Left(upgrade))
                            }
                            futures::future::Either::Right((peer_id, upgrade)) => {
                                (peer_id, futures::future::Either::Right(upgrade))
                            }
                        },
                    )
                )
            }
            pub async fn without_noise(self) -> Result<BehaviourBuilder<$runtimeCamelCase, R>,WebsocketError> {
                construct_behaviour_builder!(
                    self,
                    $dnsTcp,
                    libp2p_tls::Config::new(&self.keypair).map_err(Into::<AuthenticationError>::into)?
                )
            }
        }

        #[cfg(all(feature = $runtimeKebabCase, feature = "dns", feature = "websocket", feature = "noise"))]
        impl<T: AuthenticatedMultiplexedTransport, R>
            WebsocketNoiseBuilder<$runtimeCamelCase, T, R, WithoutTls>
        {
            pub async fn with_noise(self) -> Result<BehaviourBuilder<$runtimeCamelCase, R>, WebsocketError> {
                construct_behaviour_builder!(
                    self,
                    $dnsTcp,
                    libp2p_noise::Config::new(&self.keypair).map_err(Into::<AuthenticationError>::into)?
                )
            }
        }
    };
}

impl_websocket_noise_builder!(
    "async-std",
    AsyncStd,
    libp2p_dns::DnsConfig::system(libp2p_tcp::async_io::Transport::new(
        libp2p_tcp::Config::default(),
    ))
);
// TODO: Unnecessary await for Tokio Websocket (i.e. tokio dns). Not ideal but don't know a better way.
impl_websocket_noise_builder!(
    "tokio",
    Tokio,
    futures::future::ready(libp2p_dns::TokioDnsConfig::system(
        libp2p_tcp::tokio::Transport::new(libp2p_tcp::Config::default())
    ))
);

#[derive(Debug, thiserror::Error)]
pub enum WebsocketError {
    #[error("Dns")]
    #[cfg(any(feature = "tls", feature = "noise"))]
    Authentication(#[from] AuthenticationError),
    #[cfg(feature = "dns")]
    #[error("Dns")]
    Dns(#[from] std::io::Error),
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
        mut constructor: impl FnMut(
            &libp2p_identity::Keypair,
            libp2p_relay::client::Behaviour,
        ) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        Ok(Builder {
            behaviour: constructor(&self.keypair, self.relay_behaviour)?,
            keypair: self.keypair,
            transport: self.transport,
            phantom: PhantomData,
        })
    }
}

impl<P> BehaviourBuilder<P, NoRelayBehaviour> {
    pub fn with_behaviour<B>(
        self,
        mut constructor: impl FnMut(&libp2p_identity::Keypair) -> Result<B, Box<dyn std::error::Error>>,
    ) -> Result<Builder<P, B>, Box<dyn std::error::Error>> {
        // Discard `NoRelayBehaviour`.
        let _ = self.relay_behaviour;

        Ok(Builder {
            behaviour: constructor(&self.keypair)?,
            keypair: self.keypair,
            transport: self.transport,
            phantom: PhantomData,
        })
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
    pub fn build(self) -> libp2p_swarm::Swarm<B> {
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
    pub fn build(self) -> libp2p_swarm::Swarm<B> {
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
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "noise"))]
    fn tcp() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .with_tls()
            .with_noise()
            .unwrap()
            .with_behaviour(|_| Ok(libp2p_swarm::dummy::Behaviour))
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "noise", feature = "quic"))]
    fn tcp_quic() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .with_tls()
            .with_noise()
            .unwrap()
            .with_quic()
            .with_behaviour(|_| Ok(libp2p_swarm::dummy::Behaviour))
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "relay"
    ))]
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
            .with_tls()
            .with_noise()
            .unwrap()
            .with_relay()
            .with_tls()
            .with_noise()
            .unwrap()
            .with_behaviour(|_, relay| {
                Ok(Behaviour {
                    dummy: libp2p_swarm::dummy::Behaviour,
                    relay,
                })
            })
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "dns"
    ))]
    fn tcp_dns() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .with_tls()
            .with_noise()
            .unwrap()
            .with_dns()
            .unwrap()
            .with_behaviour(|_| Ok(libp2p_swarm::dummy::Behaviour))
            .unwrap()
            .build();
    }

    /// Showcases how to provide custom transports unknown to the libp2p crate, e.g. QUIC or WebRTC.
    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "noise"))]
    fn tcp_other_transport_other_transport() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .with_tls()
            .with_noise()
            .unwrap()
            .with_other_transport(|_| libp2p_core::transport::dummy::DummyTransport::new())
            .with_other_transport(|_| libp2p_core::transport::dummy::DummyTransport::new())
            .with_other_transport(|_| libp2p_core::transport::dummy::DummyTransport::new())
            .with_behaviour(|_| Ok(libp2p_swarm::dummy::Behaviour))
            .unwrap()
            .build();
    }

    #[tokio::test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "dns",
        feature = "websocket",
    ))]
    async fn tcp_websocket() {
        let _: libp2p_swarm::Swarm<libp2p_swarm::dummy::Behaviour> = SwarmBuilder::new()
            .with_new_identity()
            .with_tokio()
            .with_tcp()
            .with_tls()
            .with_noise()
            .unwrap()
            .with_websocket()
            .with_tls()
            .with_noise()
            .await
            .unwrap()
            .with_behaviour(|_| Ok(libp2p_swarm::dummy::Behaviour))
            .unwrap()
            .build();
    }
}
