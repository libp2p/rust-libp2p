#![doc(html_logo_url = "https://libp2p.io/img/logo_small.png")]
#![doc(html_favicon_url = "https://libp2p.io/img/favicon.png")]
#![cfg_attr(docsrs, feature(doc_cfg))]

use std::pin::Pin;
use std::sync::Arc;

use address::{dangerous_extract_tor_address, safe_extract_tor_address};
use arti_client::{TorAddrError, TorClient, TorClientBuilder};
use futures::{future::BoxFuture, FutureExt};
use libp2p_core::{transport::TransportError, Multiaddr, Transport};
use tor_rtcompat::Runtime;

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
}

#[derive(Clone)]
pub struct OnionClient<R: Runtime> {
    // client is in an Arc, because wihtout it the Transport::Dial method can't be implemented,
    // due to lifetime issues. With the, eventual, stabilization of static async traits this issue
    // will be resolved.
    client: Arc<TorClient<R>>,
    safe_mode: bool,
}

pub type OnionBuilder<R> = TorClientBuilder<R>;

impl<R: Runtime> OnionClient<R> {
    #[inline]
    pub fn from_builder(builder: OnionBuilder<R>, safe_mode: bool) -> Result<Self, OnionError> {
        let client = Arc::new(builder.create_unbootstrapped()?);
        Ok(Self { client, safe_mode })
    }

    #[inline]
    pub fn is_safe_mode(&self) -> bool {
        self.safe_mode
    }

    #[inline]
    pub fn set_safe_mode(&mut self, mode: bool) {
        self.safe_mode = mode
    }

    pub async fn bootstrap(&self) -> Result<(), OnionError> {
        self.client.bootstrap().await.map_err(OnionError::ArtiErr)
    }
}

#[cfg(all(feature = "native-tls", feature = "async-std"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "native-tls", feature = "async-std"))))]
impl OnionClient<tor_rtcompat::async_std::AsyncStdNativeTlsRuntime> {
    pub fn builder() -> OnionBuilder<tor_rtcompat::async_std::AsyncStdNativeTlsRuntime> {
        let runtime = tor_rtcompat::async_std::AsyncStdNativeTlsRuntime::current()
            .expect("Couldn't get the current async_std native-tls runtime");
        TorClient::with_runtime(runtime)
    }
}

#[cfg(all(feature = "rustls", feature = "async-std"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "rustls", feature = "async-std"))))]
impl OnionClient<tor_rtcompat::async_std::AsyncStdRustlsRuntime> {
    pub fn builder() -> OnionBuilder<tor_rtcompat::async_std::AsyncStdRustlsRuntime> {
        let runtime = tor_rtcompat::async_std::AsyncStdRustlsRuntime::current()
            .expect("Couldn't get the current async_std rustls runtime");
        TorClient::with_runtime(runtime)
    }
}

#[cfg(all(feature = "native-tls", feature = "tokio"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "native-tls", feature = "tokio"))))]
impl OnionClient<tor_rtcompat::tokio::TokioNativeTlsRuntime> {
    pub fn builder() -> OnionBuilder<tor_rtcompat::tokio::TokioNativeTlsRuntime> {
        let runtime = tor_rtcompat::tokio::TokioNativeTlsRuntime::current()
            .expect("Couldn't get the current tokio native-tls runtime");
        TorClient::with_runtime(runtime)
    }
}

#[cfg(all(feature = "rustls", feature = "tokio"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "rustls", feature = "tokio"))))]
impl OnionClient<tor_rtcompat::tokio::TokioRustlsRuntime> {
    pub fn builder() -> OnionBuilder<tor_rtcompat::tokio::TokioRustlsRuntime> {
        let runtime = tor_rtcompat::tokio::TokioRustlsRuntime::current()
            .expect("Couldn't get the current tokio rustls runtime");
        TorClient::with_runtime(runtime)
    }
}

#[cfg(all(feature = "native-tls", feature = "async-std"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "native-tls", feature = "async-std"))))]
pub type OnionAsyncStdNativeTlsClient =
    OnionClient<tor_rtcompat::async_std::AsyncStdNativeTlsRuntime>;
#[cfg(all(feature = "rustls", feature = "async-std"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "rustls", feature = "async-std"))))]
pub type OnionAsyncStdRustlsClient = OnionClient<tor_rtcompat::async_std::AsyncStdRustlsRuntime>;
#[cfg(all(feature = "native-tls", feature = "tokio"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "native-tls", feature = "tokio"))))]
pub type OnionTokioNativeTlsClient = OnionClient<tor_rtcompat::tokio::TokioNativeTlsRuntime>;
#[cfg(all(feature = "rustls", feature = "tokio"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "rustls", feature = "tokio"))))]
pub type OnionTokioRustlsClient = OnionClient<tor_rtcompat::tokio::TokioRustlsRuntime>;

#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysErrorListenerUpgrade;

impl core::future::Future for AlwaysErrorListenerUpgrade {
    type Output = Result<OnionStream, OnionError>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        panic!("onion services are not implented yet, since arti doesn't support it. (awaiting Arti 1.2.0)")
    }
}

impl<R: Runtime> Transport for OnionClient<R> {
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
        let tor_address = if !self.safe_mode {
            safe_extract_tor_address(&addr).or_else(|_| dangerous_extract_tor_address(&addr))
        } else {
            safe_extract_tor_address(&addr)
        }
        .map_err(OnionError::from)
        .map_err(TransportError::Other)?;
        let onion_client = self.client.clone();
        Ok(async move {
            onion_client
                .connect(tor_address)
                .await
                .map(OnionStream::new)
                .map_err(OnionError::ArtiErr)
        }
        .boxed())
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
