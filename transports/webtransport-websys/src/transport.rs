use futures::future::FutureExt;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::{Boxed, ListenerId, Transport as _, TransportError, TransportEvent};
use libp2p_identity::{Keypair, PeerId};
use multiaddr::Multiaddr;
use send_wrapper::SendWrapper;
use std::future::Future;
use std::pin::Pin;
use std::task::ready;
use std::task::{Context, Poll};

use crate::connection::ConnectionSend;
use crate::endpoint::Endpoint;
use crate::Connection;
use crate::Error;

/// Config for the [`Transport`].
pub struct Config {
    keypair: Keypair,
}

/// A WebTransport [`Transport`](libp2p_core::Transport) that works with `web-sys`.
pub struct Transport {
    config: Config,
}

/// Transport wrapped in [`SendWrapper`].
///
/// This is needed by Swarm. WASM is single-threaded and it is safe
/// to use [`SendWrapper`].
pub(crate) struct TransportSend {
    inner: SendWrapper<Transport>,
}

impl Config {
    /// Constructs a new configuration for the [`Transport`].
    pub fn new(keypair: &Keypair) -> Self {
        Config {
            keypair: keypair.to_owned(),
        }
    }
}

impl Transport {
    /// Constructs a new `Transport` with the given [`Config`].
    pub fn new(config: Config) -> Transport {
        Transport { config }
    }

    /// Wraps `Transport` in [`Boxed`] and makes it ready to be consumed by
    /// SwarmBuilder.
    pub fn boxed(self) -> Boxed<(PeerId, StreamMuxerBox)> {
        TransportSend::new(self)
            .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed()
    }
}

impl libp2p_core::Transport for Transport {
    type Output = (PeerId, Connection);
    type Error = Error;
    type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>>>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>>>>;

    fn listen_on(
        &mut self,
        _id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let endpoint = Endpoint::from_multiaddr(&addr).map_err(|e| match e {
            e @ Error::InvalidMultiaddr(_) => {
                log::warn!("{}", e);
                TransportError::MultiaddrNotSupported(addr)
            }
            e => TransportError::Other(e),
        })?;

        let mut session = Connection::new(&endpoint).map_err(TransportError::Other)?;
        let keypair = self.config.keypair.clone();

        Ok(async move {
            let peer_id = session
                .authenticate(&keypair, endpoint.remote_peer, endpoint.certhashes)
                .await?;
            Ok((peer_id, session))
        }
        .boxed_local())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }

    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

impl TransportSend {
    pub(crate) fn new(transport: Transport) -> Self {
        TransportSend {
            inner: SendWrapper::new(transport),
        }
    }
}

impl libp2p_core::Transport for TransportSend {
    type Output = (PeerId, ConnectionSend);
    type Error = Error;
    type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.inner.listen_on(id, addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let fut = SendWrapper::new(self.inner.dial(addr)?);

        Ok(async move {
            let (peer_id, conn) = fut.await?;
            let conn = ConnectionSend::new(conn);
            Ok((peer_id, conn))
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let fut = SendWrapper::new(self.inner.dial_as_listener(addr)?);

        Ok(async move {
            let (peer_id, conn) = fut.await?;
            let conn = ConnectionSend::new(conn);
            Ok((peer_id, conn))
        }
        .boxed())
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let inner = Pin::new(&mut *self.get_mut().inner);

        let event = ready!(inner.poll(cx)).map_upgrade(|fut| {
            let fut = SendWrapper::new(fut);

            async move {
                let (peer_id, conn) = fut.await?;
                let conn = ConnectionSend::new(conn);
                Ok((peer_id, conn))
            }
            .boxed()
        });

        Poll::Ready(event)
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(listen, observed)
    }
}
