use libp2p_core::upgrade::SelectUpgrade;
use libp2p_core::{muxing::StreamMuxerBox, Transport};
use libp2p_core::{InboundUpgrade, Negotiated, OutboundUpgrade, StreamMuxer, UpgradeInfo};
use libp2p_identity::{Keypair, PeerId};
use libp2p_swarm::{NetworkBehaviour, Swarm};
use std::convert::Infallible;
use std::io;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::bandwidth::BandwidthSinks;
use crate::builder::select_security::SelectSecurityUpgrade;
use crate::TransportExt;

#[cfg(all(
    not(target_arch = "wasm32"),
    feature = "tls",
    feature = "noise",
    any(feature = "tcp", feature = "relay", feature = "websocket")
))]
mod map;
mod select_security;

/// Build a [`Swarm`] by combining an identity, a set of [`Transport`]s and a [`NetworkBehaviour`].
///
/// ```
/// # use libp2p::{swarm::NetworkBehaviour, SwarmBuilder};
/// # use std::error::Error;
/// #
/// # #[cfg(all(
/// #     not(target_arch = "wasm32"),
/// #     feature = "tokio",
/// #     feature = "tcp",
/// #     feature = "tls",
/// #     feature = "noise",
/// #     feature = "quic",
/// #     feature = "dns",
/// #     feature = "relay",
/// #     feature = "websocket",
/// # ))]
/// # async fn build_swarm() -> Result<(), Box<dyn Error>> {
/// #     #[derive(NetworkBehaviour)]
/// #     #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// #     struct MyBehaviour {
/// #         relay: libp2p_relay::client::Behaviour,
/// #     }
///
///  let swarm = SwarmBuilder::with_new_identity()
///      .with_tokio()
///      .with_tcp()
///      .with_tls()?
///      .with_noise()?
///      .with_quic()
///      .with_dns()?
///      .with_relay()
///      .with_tls()?
///      .with_noise()?
///      .with_websocket()
///      .with_tls()?
///      .with_noise()
///      .await?
///      .with_behaviour(|_key, relay| MyBehaviour { relay })?
///      .build();
/// #
/// #     Ok(())
/// # }
/// ```
pub struct SwarmBuilder<Provider, Phase> {
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<Provider>,
    phase: Phase,
}

pub struct InitialPhase {}

impl SwarmBuilder<NoProviderSpecified, InitialPhase> {
    pub fn with_new_identity() -> SwarmBuilder<NoProviderSpecified, ProviderPhase> {
        SwarmBuilder::with_existing_identity(libp2p_identity::Keypair::generate_ed25519())
    }

    pub fn with_existing_identity(
        keypair: libp2p_identity::Keypair,
    ) -> SwarmBuilder<NoProviderSpecified, ProviderPhase> {
        SwarmBuilder {
            keypair,
            phantom: PhantomData,
            phase: ProviderPhase {},
        }
    }
}

pub struct ProviderPhase {}

impl SwarmBuilder<NoProviderSpecified, ProviderPhase> {
    #[cfg(all(not(target_arch = "wasm32"), feature = "async-std"))]
    pub fn with_async_std(self) -> SwarmBuilder<AsyncStd, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: TcpPhase {},
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub fn with_tokio(self) -> SwarmBuilder<Tokio, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: TcpPhase {},
        }
    }

    #[cfg(feature = "wasm-bindgen")]
    pub fn with_wasm_bindgen(self) -> SwarmBuilder<WasmBindgen, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: TcpPhase {},
        }
    }
}

pub struct TcpPhase {}

macro_rules! impl_tcp_builder {
    ($providerKebabCase:literal, $providerCamelCase:ident, $path:ident) => {
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
            Box<dyn std::error::Error>,
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
                                security_upgrade
                                    .into_security_upgrade(&self.keypair)
                                    .map_err(Box::new)?,
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

impl_tcp_builder!("async-std", AsyncStd, async_io);
impl_tcp_builder!("tokio", Tokio, tokio);

pub trait IntoSecurityUpgrade<C> {
    type Upgrade;
    type Error: std::error::Error + 'static;

    fn into_security_upgrade(self, keypair: &Keypair) -> Result<Self::Upgrade, Self::Error>;
}

impl<C, T, F, E> IntoSecurityUpgrade<C> for F
where
    F: for<'a> FnOnce(&'a Keypair) -> Result<T, E>,
    E: std::error::Error + 'static,
{
    type Upgrade = T;
    type Error = E;

    fn into_security_upgrade(self, keypair: &Keypair) -> Result<Self::Upgrade, Self::Error> {
        (self)(keypair)
    }
}

impl<F1, F2, C> IntoSecurityUpgrade<C> for (F1, F2)
where
    F1: IntoSecurityUpgrade<C>,
    F2: IntoSecurityUpgrade<C>,
{
    type Upgrade = SelectSecurityUpgrade<F1::Upgrade, F2::Upgrade>;
    type Error = either::Either<F1::Error, F2::Error>;

    fn into_security_upgrade(self, keypair: &Keypair) -> Result<Self::Upgrade, Self::Error> {
        let (f1, f2) = self;

        let u1 = f1
            .into_security_upgrade(keypair)
            .map_err(either::Either::Left)?;
        let u2 = f2
            .into_security_upgrade(keypair)
            .map_err(either::Either::Right)?;

        Ok(SelectSecurityUpgrade::new(u1, u2))
    }
}

pub trait IntoMultiplexerUpgrade<C> {
    type Upgrade;

    fn into_multiplexer_upgrade(self) -> Self::Upgrade;
}

impl<C, U> IntoMultiplexerUpgrade<C> for (U,) {
    type Upgrade = U;

    fn into_multiplexer_upgrade(self) -> Self::Upgrade {
        self.0
    }
}

impl<C, U1, U2> IntoMultiplexerUpgrade<C> for (U1, U2) {
    type Upgrade = SelectUpgrade<U1, U2>;

    fn into_multiplexer_upgrade(self) -> Self::Upgrade {
        SelectUpgrade::new(self.0, self.1)
    }
}

impl<Provider> SwarmBuilder<Provider, TcpPhase> {
    // TODO: This would allow one to build a faulty transport.
    fn without_tcp(
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
impl SwarmBuilder<AsyncStd, TcpPhase> {
    pub fn with_quic(
        self,
    ) -> SwarmBuilder<AsyncStd, OtherTransportPhase<impl AuthenticatedMultiplexedTransport>> {
        self.without_tcp().with_quic()
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = "tokio"))]
impl SwarmBuilder<Tokio, TcpPhase> {
    pub fn with_quic(
        self,
    ) -> SwarmBuilder<Tokio, OtherTransportPhase<impl AuthenticatedMultiplexedTransport>> {
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

    #[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
    pub fn with_websocket(
        self,
    ) -> SwarmBuilder<
        Provider,
        WebsocketTlsPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
    > {
        self.without_tcp()
            .without_quic()
            .without_any_other_transports()
            .without_dns()
            .without_relay()
            .with_websocket()
    }
}

#[cfg(any(feature = "tls", feature = "noise"))]
pub struct WithoutTls {}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct AuthenticationError(AuthenticationErrorInner);

#[derive(Debug, thiserror::Error)]
enum AuthenticationErrorInner {
    #[error("Tls")]
    #[cfg(all(not(target_arch = "wasm32"), feature = "tls"))]
    Tls(#[from] libp2p_tls::certificate::GenError),
    #[error("Noise")]
    #[cfg(feature = "noise")]
    Noise(#[from] libp2p_noise::Error),
}

pub struct QuicPhase<T> {
    transport: T,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "quic"))]
macro_rules! construct_other_transport_builder {
    ($self:ident, $quic:ident, $config:expr) => {
        SwarmBuilder {
            phase: OtherTransportPhase {
                transport: $self
                    .phase
                    .transport
                    .or_transport(
                        libp2p_quic::$quic::Transport::new($config)
                            .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer))),
                    )
                    .map(|either, _| either.into_inner()),
            },
            keypair: $self.keypair,
            phantom: PhantomData,
        }
    };
}

macro_rules! impl_quic_builder {
    ($providerKebabCase:literal, $providerCamelCase:ident, $quic:ident) => {
        #[cfg(all(not(target_arch = "wasm32"), feature = "quic", feature = $providerKebabCase))]
        impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<$providerCamelCase, QuicPhase<T>> {
            pub fn with_quic(
                self,
            ) -> SwarmBuilder<
                $providerCamelCase,
                OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
            > {
                self.with_quic_config(|key| libp2p_quic::Config::new(&key))
            }

            pub fn with_quic_config(
                self,
                constructor: impl FnOnce(&libp2p_identity::Keypair) -> libp2p_quic::Config,
            ) -> SwarmBuilder<
                $providerCamelCase,
                OtherTransportPhase<impl AuthenticatedMultiplexedTransport>,
            > {
                construct_other_transport_builder!(self, $quic, constructor(&self.keypair))
            }
        }
    };
}

impl_quic_builder!("async-std", AsyncStd, async_std);
impl_quic_builder!("tokio", Tokio, tokio);

impl<Provider, T> SwarmBuilder<Provider, QuicPhase<T>> {
    fn without_quic(self) -> SwarmBuilder<Provider, OtherTransportPhase<T>> {
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
    #[cfg(feature = "relay")]
    pub fn with_relay(self) -> SwarmBuilder<Provider, RelayTlsPhase<T>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: RelayTlsPhase {
                transport: self.phase.transport,
            },
        }
    }

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
        self.without_quic().with_other_transport(constructor)
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
    pub fn with_websocket(
        self,
    ) -> SwarmBuilder<
        Provider,
        WebsocketTlsPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
    > {
        self.without_quic()
            .without_any_other_transports()
            .without_dns()
            .without_relay()
            .with_websocket()
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
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<AsyncStd, QuicPhase<T>> {
    pub async fn with_dns(
        self,
    ) -> Result<SwarmBuilder<AsyncStd, RelayPhase<impl AuthenticatedMultiplexedTransport>>, io::Error>
    {
        self.without_quic()
            .without_any_other_transports()
            .with_dns()
            .await
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<Tokio, QuicPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<SwarmBuilder<Tokio, RelayPhase<impl AuthenticatedMultiplexedTransport>>, io::Error>
    {
        self.without_quic()
            .without_any_other_transports()
            .with_dns()
    }
}

pub struct OtherTransportPhase<T> {
    transport: T,
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

    fn without_any_other_transports(self) -> SwarmBuilder<Provider, DnsPhase<T>> {
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
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<AsyncStd, OtherTransportPhase<T>> {
    pub async fn with_dns(
        self,
    ) -> Result<SwarmBuilder<AsyncStd, RelayPhase<impl AuthenticatedMultiplexedTransport>>, io::Error>
    {
        self.without_any_other_transports().with_dns().await
    }
}
#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<Tokio, OtherTransportPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<SwarmBuilder<Tokio, RelayPhase<impl AuthenticatedMultiplexedTransport>>, io::Error>
    {
        self.without_any_other_transports().with_dns()
    }
}
#[cfg(feature = "relay")]
impl<T: AuthenticatedMultiplexedTransport, Provider>
    SwarmBuilder<Provider, OtherTransportPhase<T>>
{
    pub fn with_relay(
        self,
    ) -> SwarmBuilder<Provider, RelayTlsPhase<impl AuthenticatedMultiplexedTransport>> {
        self.without_any_other_transports()
            .without_dns()
            .with_relay()
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

pub struct DnsPhase<T> {
    transport: T,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "async-std", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<AsyncStd, DnsPhase<T>> {
    pub async fn with_dns(
        self,
    ) -> Result<SwarmBuilder<AsyncStd, RelayPhase<impl AuthenticatedMultiplexedTransport>>, io::Error>
    {
        Ok(SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: RelayPhase {
                transport: libp2p_dns::DnsConfig::system(self.phase.transport).await?,
            },
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio", feature = "dns"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<Tokio, DnsPhase<T>> {
    pub fn with_dns(
        self,
    ) -> Result<SwarmBuilder<Tokio, RelayPhase<impl AuthenticatedMultiplexedTransport>>, io::Error>
    {
        Ok(SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: RelayPhase {
                transport: libp2p_dns::TokioDnsConfig::system(self.phase.transport)?,
            },
        })
    }
}

impl<Provider, T> SwarmBuilder<Provider, DnsPhase<T>> {
    fn without_dns(self) -> SwarmBuilder<Provider, RelayPhase<T>> {
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
impl<Provider, T: AuthenticatedMultiplexedTransport> SwarmBuilder<Provider, DnsPhase<T>> {
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_dns()
            .without_relay()
            .without_websocket()
            .with_behaviour(constructor)
    }
}

pub struct RelayPhase<T> {
    transport: T,
}

// TODO: Noise feature or tls feature
#[cfg(feature = "relay")]
impl<Provider, T> SwarmBuilder<Provider, RelayPhase<T>> {
    // TODO: This should be with_relay_client.
    pub fn with_relay(self) -> SwarmBuilder<Provider, RelayTlsPhase<T>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: RelayTlsPhase {
                transport: self.phase.transport,
            },
        }
    }
}

pub struct NoRelayBehaviour;

impl<Provider, T> SwarmBuilder<Provider, RelayPhase<T>> {
    fn without_relay(self) -> SwarmBuilder<Provider, WebsocketPhase<T, NoRelayBehaviour>> {
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
    #[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
    pub fn with_websocket(
        self,
    ) -> SwarmBuilder<
        Provider,
        WebsocketTlsPhase<impl AuthenticatedMultiplexedTransport, NoRelayBehaviour>,
    > {
        self.without_relay().with_websocket()
    }

    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_relay()
            .without_websocket()
            .with_behaviour(constructor)
    }
}

#[cfg(feature = "relay")]
pub struct RelayTlsPhase<T> {
    transport: T,
}

#[cfg(feature = "relay")]
impl<Provider, T> SwarmBuilder<Provider, RelayTlsPhase<T>> {
    #[cfg(all(not(target_arch = "wasm32"), feature = "tls"))]
    pub fn with_tls(
        self,
    ) -> Result<SwarmBuilder<Provider, RelayNoisePhase<T, libp2p_tls::Config>>, AuthenticationError>
    {
        Ok(SwarmBuilder {
            phase: RelayNoisePhase {
                tls_config: libp2p_tls::Config::new(&self.keypair)
                    .map_err(|e| AuthenticationError(e.into()))?,
                transport: self.phase.transport,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        })
    }

    fn without_tls(self) -> SwarmBuilder<Provider, RelayNoisePhase<T, WithoutTls>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: RelayNoisePhase {
                tls_config: WithoutTls {},
                transport: self.phase.transport,
            },
        }
    }
}

// Shortcuts
#[cfg(all(feature = "relay", feature = "noise", feature = "async-std"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<AsyncStd, RelayTlsPhase<T>> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        SwarmBuilder<
            AsyncStd,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
        >,
        AuthenticationError,
    > {
        self.without_tls().with_noise()
    }
}
#[cfg(all(feature = "relay", feature = "noise", feature = "tokio"))]
impl<T: AuthenticatedMultiplexedTransport> SwarmBuilder<Tokio, RelayTlsPhase<T>> {
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        SwarmBuilder<
            Tokio,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
        >,
        AuthenticationError,
    > {
        self.without_tls().with_noise()
    }
}

#[cfg(feature = "relay")]
pub struct RelayNoisePhase<T, A> {
    tls_config: A,
    transport: T,
}

// TODO: Rename these macros to phase not builder. All.
#[cfg(feature = "relay")]
macro_rules! construct_websocket_builder {
    ($self:ident, $auth:expr) => {{
        let (relay_transport, relay_behaviour) =
            libp2p_relay::client::new($self.keypair.public().to_peer_id());

        SwarmBuilder {
            phase: WebsocketPhase {
                relay_behaviour,
                transport: $self
                    .phase
                    .transport
                    .or_transport(
                        relay_transport
                            .upgrade(libp2p_core::upgrade::Version::V1Lazy)
                            .authenticate($auth)
                            .multiplex(libp2p_yamux::Config::default())
                            .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
                    )
                    .map(|either, _| either.into_inner()),
            },
            keypair: $self.keypair,
            phantom: PhantomData,
        }
    }};
}

#[cfg(all(not(target_arch = "wasm32"), feature = "relay", feature = "tls"))]
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, RelayNoisePhase<T, libp2p_tls::Config>>
{
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        SwarmBuilder<
            Provider,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
        >,
        AuthenticationError,
    > {
        Ok(construct_websocket_builder!(
            self,
            map::Upgrade::new(
                libp2p_core::upgrade::SelectUpgrade::new(
                    self.phase.tls_config,
                    libp2p_noise::Config::new(&self.keypair)
                        .map_err(|e| AuthenticationError(e.into()))?,
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
        ))
    }

    fn without_noise(
        self,
    ) -> SwarmBuilder<
        Provider,
        WebsocketPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
    > {
        construct_websocket_builder!(self, self.phase.tls_config)
    }
}

#[cfg(feature = "relay")]
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, RelayNoisePhase<T, WithoutTls>>
{
    #[cfg(feature = "noise")]
    pub fn with_noise(
        self,
    ) -> Result<
        SwarmBuilder<
            Provider,
            WebsocketPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
        >,
        AuthenticationError,
    > {
        let _ = self.phase.tls_config;

        Ok(construct_websocket_builder!(
            self,
            libp2p_noise::Config::new(&self.keypair).map_err(|e| AuthenticationError(e.into()))?
        ))
    }
}

// Shortcuts
#[cfg(all(not(target_arch = "wasm32"), feature = "tls", feature = "relay"))]
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, RelayNoisePhase<T, libp2p_tls::Config>>
{
    #[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
    pub fn with_websocket(
        self,
    ) -> SwarmBuilder<
        Provider,
        WebsocketTlsPhase<impl AuthenticatedMultiplexedTransport, libp2p_relay::client::Behaviour>,
    > {
        self.without_noise().with_websocket()
    }

    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> R,
    ) -> Result<
        SwarmBuilder<Provider, SwarmPhase<impl AuthenticatedMultiplexedTransport, B>>,
        R::Error,
    > {
        self.without_noise()
            .without_websocket()
            .with_behaviour(constructor)
    }
}

pub struct WebsocketPhase<T, R> {
    transport: T,
    relay_behaviour: R,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
impl<Provider, T, R> SwarmBuilder<Provider, WebsocketPhase<T, R>> {
    pub fn with_websocket(self) -> SwarmBuilder<Provider, WebsocketTlsPhase<T, R>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketTlsPhase {
                transport: self.phase.transport,
                relay_behaviour: self.phase.relay_behaviour,
            },
        }
    }
}

impl<Provider, T: AuthenticatedMultiplexedTransport, R>
    SwarmBuilder<Provider, WebsocketPhase<T, R>>
{
    fn without_websocket(self) -> SwarmBuilder<Provider, BandwidthLoggingPhase<T, R>> {
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

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
pub struct WebsocketTlsPhase<T, R> {
    transport: T,
    relay_behaviour: R,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
impl<Provider, T, R> SwarmBuilder<Provider, WebsocketTlsPhase<T, R>> {
    #[cfg(feature = "tls")]
    pub fn with_tls(
        self,
    ) -> Result<
        SwarmBuilder<Provider, WebsocketNoisePhase<T, R, libp2p_tls::Config>>,
        AuthenticationError,
    > {
        Ok(SwarmBuilder {
            phase: WebsocketNoisePhase {
                tls_config: libp2p_tls::Config::new(&self.keypair)
                    .map_err(|e| AuthenticationError(e.into()))?,
                relay_behaviour: self.phase.relay_behaviour,
                transport: self.phase.transport,
                phantom: PhantomData,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        })
    }

    fn without_tls(self) -> SwarmBuilder<Provider, WebsocketNoisePhase<T, R, WithoutTls>> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: WebsocketNoisePhase {
                tls_config: WithoutTls {},
                relay_behaviour: self.phase.relay_behaviour,
                transport: self.phase.transport,
                phantom: PhantomData,
            },
        }
    }
}

// Shortcuts
#[cfg(all(
    not(target_arch = "wasm32"),
    feature = "websocket",
    feature = "noise",
    feature = "async-std"
))]
impl<T: AuthenticatedMultiplexedTransport, R> SwarmBuilder<AsyncStd, WebsocketTlsPhase<T, R>> {
    #[cfg(feature = "noise")]
    pub async fn with_noise(
        self,
    ) -> Result<
        SwarmBuilder<AsyncStd, BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, R>>,
        WebsocketError,
    > {
        self.without_tls().with_noise().await
    }
}
#[cfg(all(
    not(target_arch = "wasm32"),
    feature = "websocket",
    feature = "noise",
    feature = "tokio"
))]
impl<T: AuthenticatedMultiplexedTransport, R> SwarmBuilder<Tokio, WebsocketTlsPhase<T, R>> {
    #[cfg(feature = "noise")]
    pub async fn with_noise(
        self,
    ) -> Result<
        SwarmBuilder<Tokio, BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, R>>,
        WebsocketError,
    > {
        self.without_tls().with_noise().await
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
pub struct WebsocketNoisePhase<T, R, A> {
    tls_config: A,
    transport: T,
    relay_behaviour: R,
    phantom: PhantomData<A>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
macro_rules! construct_behaviour_builder {
    ($self:ident, $dnsTcp:expr, $auth:expr) => {{
        let websocket_transport =
            libp2p_websocket::WsConfig::new($dnsTcp.await.map_err(|e| WebsocketError(e.into()))?)
                .upgrade(libp2p_core::upgrade::Version::V1)
                .authenticate($auth)
                .multiplex(libp2p_yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)));

        Ok(SwarmBuilder {
            keypair: $self.keypair,
            phantom: PhantomData,
            phase: BandwidthLoggingPhase {
                transport: websocket_transport
                    .or_transport($self.phase.transport)
                    .map(|either, _| either.into_inner()),
                relay_behaviour: $self.phase.relay_behaviour,
            },
        })
    }};
}

macro_rules! impl_websocket_noise_builder {
    ($providerKebabCase:literal, $providerCamelCase:ident, $dnsTcp:expr) => {
        #[cfg(all(
            not(target_arch = "wasm32"),
            feature = $providerKebabCase,
            feature = "websocket",
            feature = "dns",
            feature = "tls"
        ))]
        impl<T: AuthenticatedMultiplexedTransport, R>
            SwarmBuilder<$providerCamelCase, WebsocketNoisePhase< T, R, libp2p_tls::Config>>
        {
            #[cfg(feature = "noise")]
            pub async fn with_noise(self) -> Result<SwarmBuilder<$providerCamelCase, BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, R>>, WebsocketError> {
                construct_behaviour_builder!(
                    self,
                    $dnsTcp,
                    map::Upgrade::new(
                        libp2p_core::upgrade::SelectUpgrade::new(
                            self.phase.tls_config,
                            libp2p_noise::Config::new(&self.keypair).map_err(|e| WebsocketError(AuthenticationErrorInner::from(e).into()))?,
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
            pub async fn without_noise(self) -> Result<SwarmBuilder<$providerCamelCase, BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, R>>, WebsocketError> {
                construct_behaviour_builder!(
                    self,
                    $dnsTcp,
                    libp2p_tls::Config::new(&self.keypair).map_err(|e| WebsocketError(AuthenticationErrorInner::from(e).into()))?
                )
            }
        }

        #[cfg(all(not(target_arch = "wasm32"), feature = $providerKebabCase, feature = "dns", feature = "websocket", feature = "noise"))]
        impl<T: AuthenticatedMultiplexedTransport, R>
            SwarmBuilder<$providerCamelCase, WebsocketNoisePhase< T, R, WithoutTls>>
        {
            pub async fn with_noise(self) -> Result<SwarmBuilder<$providerCamelCase, BandwidthLoggingPhase<impl AuthenticatedMultiplexedTransport, R>>, WebsocketError> {
                let _ = self.phase.tls_config;

                construct_behaviour_builder!(
                    self,
                    $dnsTcp,
                    libp2p_noise::Config::new(&self.keypair).map_err(|e| WebsocketError(AuthenticationErrorInner::from(e).into()))?
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
#[error(transparent)]
pub struct WebsocketError(WebsocketErrorInner);

#[derive(Debug, thiserror::Error)]
enum WebsocketErrorInner {
    #[error("Dns")]
    #[cfg(any(feature = "tls", feature = "noise"))]
    Authentication(#[from] AuthenticationErrorInner),
    #[cfg(feature = "dns")]
    #[error("Dns")]
    Dns(#[from] io::Error),
}

pub struct BandwidthLoggingPhase<T, R> {
    relay_behaviour: R,
    transport: T,
}

impl<T: AuthenticatedMultiplexedTransport, Provider, R>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, R>>
{
    pub fn with_bandwidth_logging(
        self,
    ) -> (
        SwarmBuilder<Provider, BehaviourPhase<impl AuthenticatedMultiplexedTransport, R>>,
        Arc<BandwidthSinks>,
    ) {
        let (transport, sinks) = self.phase.transport.with_bandwidth_logging();
        (
            SwarmBuilder {
                phase: BehaviourPhase {
                    relay_behaviour: self.phase.relay_behaviour,
                    transport,
                },
                keypair: self.keypair,
                phantom: PhantomData,
            },
            sinks,
        )
    }

    pub fn without_bandwidth_logging(self) -> SwarmBuilder<Provider, BehaviourPhase<T, R>> {
        SwarmBuilder {
            phase: BehaviourPhase {
                relay_behaviour: self.phase.relay_behaviour,
                transport: self.phase.transport,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

// Shortcuts
#[cfg(feature = "relay")]
impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, libp2p_relay::client::Behaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_logging().with_behaviour(constructor)
    }
}

impl<Provider, T: AuthenticatedMultiplexedTransport>
    SwarmBuilder<Provider, BandwidthLoggingPhase<T, NoRelayBehaviour>>
{
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        self.without_bandwidth_logging().with_behaviour(constructor)
    }
}

pub struct BehaviourPhase<T, R> {
    relay_behaviour: R,
    transport: T,
}

#[cfg(feature = "relay")]
impl<T, Provider> SwarmBuilder<Provider, BehaviourPhase<T, libp2p_relay::client::Behaviour>> {
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair, libp2p_relay::client::Behaviour) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        Ok(SwarmBuilder {
            phase: SwarmPhase {
                behaviour: constructor(&self.keypair, self.phase.relay_behaviour)
                    .try_into_behaviour()?,
                transport: self.phase.transport,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        })
    }
}

impl<T, Provider> SwarmBuilder<Provider, BehaviourPhase<T, NoRelayBehaviour>> {
    pub fn with_behaviour<B, R: TryIntoBehaviour<B>>(
        self,
        constructor: impl FnOnce(&libp2p_identity::Keypair) -> R,
    ) -> Result<SwarmBuilder<Provider, SwarmPhase<T, B>>, R::Error> {
        // Discard `NoRelayBehaviour`.
        let _ = self.phase.relay_behaviour;

        Ok(SwarmBuilder {
            phase: SwarmPhase {
                behaviour: constructor(&self.keypair).try_into_behaviour()?,
                transport: self.phase.transport,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        })
    }
}

pub struct SwarmPhase<T, B> {
    behaviour: B,
    transport: T,
}

impl<T, B, Provider> SwarmBuilder<Provider, SwarmPhase<T, B>> {
    pub fn with_swarm_config(
        self,
        config: libp2p_swarm::SwarmConfig,
    ) -> SwarmBuilder<Provider, BuildPhase<T, B>> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: config,
            },
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "async-std"))]
impl<T: AuthenticatedMultiplexedTransport, B: NetworkBehaviour>
    SwarmBuilder<AsyncStd, SwarmPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: libp2p_swarm::SwarmConfig::with_async_std_executor(),
            },
            keypair: self.keypair,
            phantom: PhantomData::<AsyncStd>,
        }
        .build()
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
impl<T: AuthenticatedMultiplexedTransport, B: NetworkBehaviour>
    SwarmBuilder<Tokio, SwarmPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: libp2p_swarm::SwarmConfig::with_tokio_executor(),
            },
            keypair: self.keypair,
            phantom: PhantomData::<Tokio>,
        }
        .build()
    }
}

#[cfg(feature = "wasm-bindgen")]
impl<T: AuthenticatedMultiplexedTransport, B: NetworkBehaviour>
    SwarmBuilder<WasmBindgen, SwarmPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        SwarmBuilder {
            phase: BuildPhase {
                behaviour: self.phase.behaviour,
                transport: self.phase.transport,
                swarm_config: libp2p_swarm::SwarmConfig::with_wasm_executor(),
            },
            keypair: self.keypair,
            phantom: PhantomData::<WasmBindgen>,
        }
        .build()
    }
}

pub struct BuildPhase<T, B> {
    behaviour: B,
    transport: T,
    swarm_config: libp2p_swarm::SwarmConfig,
}

const CONNECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl<Provider, T: AuthenticatedMultiplexedTransport, B: libp2p_swarm::NetworkBehaviour>
    SwarmBuilder<Provider, BuildPhase<T, B>>
{
    pub fn build(self) -> Swarm<B> {
        Swarm::new_with_config(
            libp2p_core::transport::timeout::TransportTimeout::new(
                self.phase.transport,
                CONNECTION_TIMEOUT,
            )
            .boxed(),
            self.phase.behaviour,
            self.keypair.public().to_peer_id(),
            self.phase.swarm_config,
        )
    }
}

pub enum NoProviderSpecified {}

#[cfg(feature = "async-std")]
pub enum AsyncStd {}

#[cfg(feature = "tokio")]
pub enum Tokio {}

#[cfg(feature = "wasm-bindgen")]
pub enum WasmBindgen {}

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

// TODO: Seal this.
pub trait TryIntoBehaviour<B> {
    type Error;

    fn try_into_behaviour(self) -> Result<B, Self::Error>;
}

impl<B> TryIntoBehaviour<B> for B
where
    B: NetworkBehaviour,
{
    type Error = Infallible;

    fn try_into_behaviour(self) -> Result<B, Self::Error> {
        Ok(self)
    }
}

impl<B> TryIntoBehaviour<B> for Result<B, Box<dyn std::error::Error + Send + Sync>>
where
    B: NetworkBehaviour,
{
    // TODO mxinden: why do we need an io error here? isn't box<dyn> enough?
    type Error = io::Error; // TODO: Consider a dedicated type here with a descriptive message like "failed to build behaviour"?

    fn try_into_behaviour(self) -> Result<B, Self::Error> {
        self.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

// TODO: Seal this.
pub trait TryIntoTransport<T> {
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
    // TODO mxinden: why do we need an io error here? isn't box<dyn> enough?
    type Error = io::Error; // TODO: Consider a dedicated type here with a descriptive message like "failed to build behaviour"?

    fn try_into_transport(self) -> Result<T, Self::Error> {
        self.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "noise"))]
    fn tcp() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                // TODO: The single tuple is not intuitive.
                (libp2p_yamux::Config::default(),),
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(
        feature = "async-std",
        feature = "tcp",
        feature = "tls",
        feature = "noise"
    ))]
    fn async_std_tcp() {
        let _ = SwarmBuilder::with_new_identity()
            .with_async_std()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                // TODO: The single tuple is not intuitive.
                (libp2p_yamux::Config::default(),),
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "mplex"))]
    fn tcp_yamux_mplex() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                (
                    libp2p_yamux::Config::default(),
                    libp2p_mplex::MplexConfig::default(),
                ),
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "noise"))]
    fn tcp_tls_noise() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                (
                    libp2p_yamux::Config::default(),
                    libp2p_mplex::MplexConfig::default(),
                ),
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "quic"
    ))]
    fn tcp_quic() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                (libp2p_yamux::Config::default(),),
            )
            .unwrap()
            .with_quic()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
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

        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                (libp2p_yamux::Config::default(),),
            )
            .unwrap()
            .with_relay()
            .with_tls()
            .unwrap()
            .with_noise()
            .unwrap()
            .with_behaviour(|_, relay| Behaviour {
                dummy: libp2p_swarm::dummy::Behaviour,
                relay,
            })
            .unwrap()
            .build();
    }

    #[tokio::test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "dns"
    ))]
    async fn tcp_dns() {
        SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                (libp2p_yamux::Config::default(),),
            )
            .unwrap()
            .with_dns()
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    /// Showcases how to provide custom transports unknown to the libp2p crate, e.g. WebRTC.
    #[test]
    #[cfg(feature = "tokio")]
    fn other_transport() -> Result<(), Box<dyn std::error::Error>> {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            // Closure can either return a Transport directly.
            .with_other_transport(|_| libp2p_core::transport::dummy::DummyTransport::new())?
            // Or a Result containing a Transport.
            .with_other_transport(|_| {
                if true {
                    Ok(libp2p_core::transport::dummy::DummyTransport::new())
                } else {
                    Err(Box::from("test"))
                }
            })?
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();

        Ok(())
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
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                (libp2p_yamux::Config::default(),),
            )
            .unwrap()
            .with_websocket()
            .with_tls()
            .unwrap()
            .with_noise()
            .await
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[tokio::test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "quic",
        feature = "dns",
        feature = "relay",
        feature = "websocket",
    ))]
    async fn all() {
        #[derive(NetworkBehaviour)]
        #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
        struct MyBehaviour {
            relay: libp2p_relay::client::Behaviour,
        }

        let (builder, _bandwidth_sinks) = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                (libp2p_yamux::Config::default(),),
            )
            .unwrap()
            .with_quic()
            .with_dns()
            .unwrap()
            .with_relay()
            .with_tls()
            .unwrap()
            .with_noise()
            .unwrap()
            .with_websocket()
            .with_tls()
            .unwrap()
            .with_noise()
            .await
            .unwrap()
            .with_bandwidth_logging();
        let _: Swarm<MyBehaviour> = builder
            .with_behaviour(|_key, relay| MyBehaviour { relay })
            .unwrap()
            .build();
    }
}
