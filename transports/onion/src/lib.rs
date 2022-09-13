#[cfg(all(feature = "async-std", feature = "tokio"))]
compile_error!("The features `async-std` and `tokio` are mutually exclusive");

// this is somewhat unnecessary, since arti-client won't compile before this is evaluated.
#[cfg(not(any(feature = "async-std", feature = "tokio")))]
compile_error!("Either one of the features `async-std` or `tokio` have to be enabled");

#[cfg(all(feature = "native-tls", feature = "rustls"))]
compile_error!("The features `native-tls` and `tokio` are mutually exclusive");

use core::pin::Pin;
use std::sync::Arc;

use address::dangerous_extract_tor_address;
use arti_client::{TorAddrError, TorClient, TorClientBuilder};
use futures::{future::BoxFuture, FutureExt};
use libp2p_core::{transport::TransportError, Multiaddr, Transport};
use tor_rtcompat::PreferredRuntime;

mod address;

#[cfg(feature = "async-std")]
pub mod async_io;
#[cfg(feature = "async-std")]
pub use crate::async_io::OnionStream;

#[cfg(feature = "tokio")]
pub mod tokio;
#[cfg(feature = "tokio")]
pub use crate::tokio::OnionStream;

#[derive(Debug, thiserror::Error)]
pub enum OnionError {
    #[error("error during address translation")]
    AddrErr(#[from] TorAddrError),
    #[error("error in arti")]
    ArtiErr(#[from] arti_client::Error),
    #[error("onion services are not implented yet, since arti doesn't support it. (awaiting Arti 1.2.0)")]
    OnionServiceUnimplemented,
}

pub struct OnionClient {
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

    fn listen_on(
        &mut self,
        _addr: libp2p_core::Multiaddr,
    ) -> Result<
        libp2p_core::transport::ListenerId,
        libp2p_core::transport::TransportError<Self::Error>,
    > {
        Err(TransportError::Other(OnionError::OnionServiceUnimplemented))
    }

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

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.dial(addr)
    }

    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>>
    {
        std::task::Poll::Pending
    }
}
