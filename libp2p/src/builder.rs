use libp2p_core::{muxing::StreamMuxerBox, Transport};
use std::marker::PhantomData;

pub struct TransportBuilder {
    keypair: libp2p_identity::Keypair,
}

impl TransportBuilder {
    pub fn new(keypair: libp2p_identity::Keypair) -> Self {
        Self { keypair }
    }

    pub fn with_async_std(self) -> TcpBuilder<AsyncStd> {
        TcpBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    pub fn with_tokio(self) -> TcpBuilder<Tokio> {
        TcpBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub enum AsyncStd {}
pub enum Tokio {}

pub struct TcpBuilder<P> {
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

impl TcpBuilder<AsyncStd> {
    pub fn with_tcp(self) -> DnsBuilder<AsyncStd, impl Transport> {
        DnsBuilder {
            tcp: libp2p_tcp::async_io::Transport::new(libp2p_tcp::Config::new().nodelay(true))
                .upgrade(libp2p_core::upgrade::Version::V1)
                .authenticate(libp2p_noise::Config::new(&self.keypair).unwrap())
                .multiplex(libp2p_yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c))),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub struct DnsBuilder<P, Tcp> {
    tcp: Tcp,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

impl<T> DnsBuilder<AsyncStd, T>
where
    T: Transport + Send + Unpin + 'static,
{
    pub async fn with_dns(self) -> RelayBuilder<AsyncStd, libp2p_dns::DnsConfig<T>> {
        RelayBuilder {
            transport: libp2p_dns::DnsConfig::system(self.tcp)
                .await
                .expect("TODO: Handle"),
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }

    pub fn without_dns(self) -> RelayBuilder<AsyncStd, T> {
        RelayBuilder {
            transport: self.tcp,
            keypair: self.keypair,
            phantom: PhantomData,
        }
    }
}

pub struct RelayBuilder<P, T> {
    transport: T,
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<P>,
}

impl<P, T> RelayBuilder<P, T>
where
    T: Transport + Send + Unpin + 'static,
    <T as Transport>::Error: Send + 'static,
    <T as Transport>::Dial: Send,
{
    pub fn with_relay(
        self,
        relay_transport: libp2p_relay::client::Transport,
    ) -> Builder<impl Transport> {
        Builder {
            transport: self.transport.or_transport(relay_transport),
        }
    }
}

pub struct Builder<T> {
    transport: T,
}

impl<T> Builder<T>
where
    T: Transport + Send + Unpin + 'static,
    <T as Transport>::Error: Send + Sync + 'static,
    <T as Transport>::Dial: Send,
    <T as Transport>::ListenerUpgrade: Send,
{
    pub fn build(self) -> libp2p_core::transport::Boxed<<T as Transport>::Output> {
        self.transport.boxed()
    }
}
