#![doc(html_logo_url = "https://libp2p.io/img/logo_small.png")]
#![doc(html_favicon_url = "https://libp2p.io/img/favicon.png")]

use std::pin::Pin;
use std::sync::Arc;

use address::dangerous_extract_tor_address;
use arti_client::{TorAddrError, TorClient, TorClientBuilder};
use futures::{future::BoxFuture, FutureExt};
use libp2p_core::{transport::TransportError, Multiaddr, Transport};
use tor_rtcompat::PreferredRuntime;

mod address;
mod provider;

#[doc(inline)]
pub use provider::OnionStream;

#[derive(Debug, thiserror::Error)]
pub enum OnionError {
    #[error("error during address translation")]
    AddrErr(#[from] TorAddrError),
    #[error("error in arti")]
    ArtiErr(#[from] arti_client::Error),
    #[error("onion services are not implented yet, since arti doesn't support it. (awaiting Arti 1.2.0)")]
    OnionServiceUnimplemented,
}

#[derive(Clone)]
pub struct OnionClient {
    // client is in an Arc, because wihtout it the Transport::Dial method can't be implemented,
    // due to lifetime issues. With the, eventual, stabilization of static async traits this issue
    // will be resolved.
    client: Arc<TorClient<PreferredRuntime>>,
}

pub type OnionBuilder = TorClientBuilder<PreferredRuntime>;

impl OnionClient {
    #[inline]
    pub fn builder() -> OnionBuilder {
        TorClient::builder()
    }

    #[inline]
    pub fn from_builder(builder: OnionBuilder) -> Result<Self, OnionError> {
        let client = Arc::new(builder.create_unbootstrapped()?);
        Ok(Self { client })
    }

    pub async fn bootstrap(&self) -> Result<(), OnionError> {
        Ok(self.client.bootstrap().await?)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysErrorListenerUpgrade;

impl core::future::Future for AlwaysErrorListenerUpgrade {
    type Output = Result<OnionStream, OnionError>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        core::task::Poll::Ready(Err(OnionError::OnionServiceUnimplemented))
    }
}

impl Transport for OnionClient {
    type Output = OnionStream;
    type Error = OnionError;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type ListenerUpgrade = AlwaysErrorListenerUpgrade;

    /// Always returns `TransportError::MultiaddrNotSupported`
    fn listen_on(
        &mut self,
        addr: libp2p_core::Multiaddr,
    ) -> Result<
        libp2p_core::transport::ListenerId,
        libp2p_core::transport::TransportError<Self::Error>,
    > {
        // although this address might be supported, this is returned in order to not provoke an
        // error when trying to listen on this transport.
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    /// Always returns false
    fn remove_listener(&mut self, _id: libp2p_core::transport::ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let tor_address = dangerous_extract_tor_address(&addr)
            .map_err(OnionError::from)
            .map_err(TransportError::Other)?;
        let onion_client = self.client.clone();
        Ok(async move { Ok(OnionStream::new(onion_client.connect(tor_address).await?)) }.boxed())
    }

    /// Equivalent to `Transport::dial`
    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.dial(addr)
    }

    /// always returns `None`
    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }

    /// always returns pending
    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>>
    {
        // pending is returned here, because this won't panic an OrTransport.
        std::task::Poll::Pending
    }
}
