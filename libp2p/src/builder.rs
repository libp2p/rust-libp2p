use libp2p_core::{muxing::StreamMuxerBox, Transport};
use std::marker::PhantomData;

pub struct TransportBuilder {
    keypair: libp2p_identity::Keypair,
}

impl TransportBuilder {
    pub fn new(keypair: libp2p_identity::Keypair) -> Self {
        Self { keypair }
    }

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
    pub fn with_tcp(self) -> RelayBuilder<AsyncStd, impl BuildableTransport> {
        RelayBuilder {
            transport: libp2p_tcp::async_io::Transport::new(
                libp2p_tcp::Config::new().nodelay(true),
            )
            .upgrade(libp2p_core::upgrade::Version::V1)
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
    pub fn with_tcp(self) -> RelayBuilder<Tokio, impl BuildableTransport> {
        RelayBuilder {
            transport: libp2p_tcp::tokio::Transport::new(libp2p_tcp::Config::new().nodelay(true))
                .upgrade(libp2p_core::upgrade::Version::V1)
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
impl<P, T: BuildableTransport> RelayBuilder<P, T> {
    pub fn with_relay(
        self,
        relay_transport: libp2p_relay::client::Transport,
    ) -> OtherTransportBuilder<P, impl BuildableTransport> {
        OtherTransportBuilder {
            transport: self
                .transport
                .or_transport(
                    relay_transport
                        .upgrade(libp2p_core::upgrade::Version::V1)
                        .authenticate(libp2p_noise::Config::new(&self.keypair).unwrap())
                        .multiplex(libp2p_yamux::Config::default())
                        .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
                )
                .map(|either, _| either.into_inner()),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

impl<P, T> RelayBuilder<P, T> {
    pub fn without_relay(self) -> OtherTransportBuilder<P, T> {
        OtherTransportBuilder {
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub struct OtherTransportBuilder<P, T> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

impl<P, T: BuildableTransport> OtherTransportBuilder<P, T> {
    pub fn with_other_transport(
        self,
        // TODO: could as well be a closure that takes keypair and maybe provider?
        transport: impl BuildableTransport,
    ) -> OtherTransportBuilder<P, impl BuildableTransport> {
        OtherTransportBuilder {
            transport: self
                .transport
                .or_transport(transport)
                .map(|either, _| either.into_inner()),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    // TODO: Not the ideal name.
    pub fn no_more_other_transports(self) -> DnsBuilder<P, impl BuildableTransport> {
        DnsBuilder {
            transport: self.transport,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub struct DnsBuilder<P, T> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

#[cfg(all(feature = "async-std", feature = "dns"))]
impl<T> DnsBuilder<AsyncStd, T> {
    pub async fn with_dns(self) -> Builder<libp2p_dns::DnsConfig<T>> {
        Builder {
            transport: libp2p_dns::DnsConfig::system(self.transport)
                .await
                .expect("TODO: Handle"),
        }
    }
}

#[cfg(all(feature = "tokio", feature = "dns"))]
impl<T> DnsBuilder<Tokio, T> {
    pub fn with_dns(self) -> Builder<libp2p_dns::TokioDnsConfig<T>> {
        Builder {
            transport: libp2p_dns::TokioDnsConfig::system(self.transport).expect("TODO: Handle"),
        }
    }
}

impl<P, T> DnsBuilder<P, T> {
    pub fn without_dns(self) -> Builder<T> {
        Builder {
            transport: self.transport,
        }
    }
}

pub struct Builder<T> {
    transport: T,
}

impl<T: BuildableTransport> Builder<T> {
    pub fn build(self) -> libp2p_core::transport::Boxed<<T as Transport>::Output> {
        self.transport.boxed()
    }
}

#[cfg(feature = "async-std")]
pub enum AsyncStd {}

#[cfg(feature = "tokio")]
pub enum Tokio {}

pub trait BuildableTransport:
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

impl<T> BuildableTransport for T
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
        let key = libp2p_identity::Keypair::generate_ed25519();
        let _: libp2p_core::transport::Boxed<(libp2p_identity::PeerId, StreamMuxerBox)> =
            TransportBuilder::new(key)
                .with_tokio()
                .with_tcp()
                .without_relay()
                .no_more_other_transports()
                .without_dns()
                .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "relay"))]
    fn tcp_relay() {
        let key = libp2p_identity::Keypair::generate_ed25519();
        let (relay_transport, _) = libp2p_relay::client::new(key.public().to_peer_id());

        let _: libp2p_core::transport::Boxed<(libp2p_identity::PeerId, StreamMuxerBox)> =
            TransportBuilder::new(key)
                .with_tokio()
                .with_tcp()
                .with_relay(relay_transport)
                .no_more_other_transports()
                .without_dns()
                .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "dns"))]
    fn tcp_dns() {
        let key = libp2p_identity::Keypair::generate_ed25519();
        let _: libp2p_core::transport::Boxed<(libp2p_identity::PeerId, StreamMuxerBox)> =
            TransportBuilder::new(key)
                .with_tokio()
                .with_tcp()
                .without_relay()
                .no_more_other_transports()
                .with_dns()
                .build();
    }

    /// Showcases how to provide custom transports unknown to the libp2p crate, e.g. QUIC or WebRTC.
    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp"))]
    fn tcp_other_transport_other_transport() {
        let key = libp2p_identity::Keypair::generate_ed25519();
        let _: libp2p_core::transport::Boxed<(libp2p_identity::PeerId, StreamMuxerBox)> =
            TransportBuilder::new(key)
                .with_tokio()
                .with_tcp()
                .without_relay()
                .with_other_transport(libp2p_core::transport::dummy::DummyTransport::new())
                .with_other_transport(libp2p_core::transport::dummy::DummyTransport::new())
                .with_other_transport(libp2p_core::transport::dummy::DummyTransport::new())
                .no_more_other_transports()
                .without_dns()
                .build();
    }
}
