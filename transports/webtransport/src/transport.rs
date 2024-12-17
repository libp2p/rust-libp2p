use std::collections::HashSet;
use std::future::Pending;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures::{prelude::*, stream::SelectAll};
use futures::future::BoxFuture;
use if_watch::IfEvent;
use if_watch::tokio::IfWatcher;
use wtransport::config::TlsServerConfig;
use wtransport::endpoint::{endpoint_side, IncomingSession};
use wtransport::error::ConnectionError;
use wtransport::ServerConfig;

use libp2p_core::{Multiaddr, Transport};
use libp2p_core::multiaddr::Protocol;
use libp2p_core::multihash::Multihash;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::{DialOpts, ListenerId, TransportError, TransportEvent};
use libp2p_identity::{Keypair, PeerId};

use crate::Connecting;
use crate::certificate::CertHash;
use crate::config::Config;
use crate::connection::Connection;
use crate::Error;

pub struct GenTransport {
    server_tls_config: TlsServerConfig,
    keypair: Keypair,
    cert_hashes: Vec<CertHash>,
    /// Timeout for the [`Connecting`] future.
    handshake_timeout: Duration,

    listeners: SelectAll<Listener>,
    /// Waker to poll the transport again when a new listener is added.
    waker: Option<Waker>,
}

impl GenTransport {
    pub fn new(config: Config) -> Self {
        let handshake_timeout = config.handshake_timeout;
        let keypair = config.keypair.clone();
        let cert_hashes = config.cert_hashes();
        let server_tls_config = config.server_tls_config;

        GenTransport {
            server_tls_config,
            keypair,
            cert_hashes,
            handshake_timeout,
            listeners: SelectAll::new(),
            waker: None,
        }
    }

    /// Extract the addr, quic version and peer id from the given [`Multiaddr`].
    fn remote_multiaddr_to_socketaddr(
        &self,
        addr: Multiaddr,
        check_unspecified_addr: bool,
    ) -> Result<
        (SocketAddr, Option<PeerId>),
        TransportError<<Self as Transport>::Error>,
    > {
        let (socket_addr, peer_id) = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if check_unspecified_addr && (socket_addr.port() == 0 || socket_addr.ip().is_unspecified())
        {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        Ok((socket_addr, peer_id))
    }
}

impl Transport for GenTransport {
    type Output = (PeerId, Connection);
    type Error = Error;
    type ListenerUpgrade = Connecting;
    type Dial = Pending<Result<Self::Output, Self::Error>>;

    fn listen_on(&mut self, id: ListenerId, addr: Multiaddr) -> Result<(), TransportError<Self::Error>> {
        let (socket_addr, _peer_id) = self.remote_multiaddr_to_socketaddr(addr, false)?;
        let config = ServerConfig::builder()
            .with_bind_address(socket_addr.clone())
            .with_custom_tls(self.server_tls_config.clone())
            .build();

        let endpoint = wtransport::Endpoint::server(config)
            .map_err(|e| { TransportError::Other(e.into()) })?;
        let keypair = &self.keypair;
        let cert_hashes = &self.cert_hashes;
        let handshake_timeout = self.handshake_timeout.clone();

        let listener = Listener::new(
            id, socket_addr, Arc::new(endpoint), keypair, cert_hashes, handshake_timeout,
        )?;
        self.listeners.push(listener);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        todo!()
    }

    fn dial(&mut self, _addr: Multiaddr, _opts: DialOpts) -> Result<Self::Dial, TransportError<Self::Error>> {
        panic!("Dial operation is not supported!")
    }

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        todo!()
    }
}

/// Listener for incoming connections.
struct Listener {
    /// Id of the listener.
    listener_id: ListenerId,
    /// Endpoint
    endpoint: Arc<wtransport::Endpoint<endpoint_side::Server>>,
    /// A future to poll new incoming connections.
    accept: BoxFuture<'static, IncomingSession>,
    /// Timeout for connection establishment on inbound connections.
    handshake_timeout: Duration,
    /// Watcher for network interface changes.
    ///
    /// None if we are only listening on a single interface.
    if_watcher: Option<IfWatcher>,
    /// Whether the listener was closed and the stream should terminate.
    is_closed: bool,
    /// Pending event to reported.
    pending_event: Option<<Self as Stream>::Item>,
    /// The stream must be to awaken after it has been closed to deliver the last event.
    close_listener_waker: Option<Waker>,

    listening_addresses: HashSet<IpAddr>,

    keypair: Keypair,
    cert_hashes: Vec<CertHash>,
}

impl Listener {
    fn new(
        listener_id: ListenerId,
        socket_addr: SocketAddr,
        endpoint: Arc<wtransport::Endpoint<endpoint_side::Server>>,
        keypair: &Keypair,
        cert_hashes: &Vec<CertHash>,
        handshake_timeout: Duration, ) -> Result<Self, Error> {
        let if_watcher;
        let pending_event;
        let mut listening_addresses = HashSet::new();
        if socket_addr.ip().is_unspecified() {
            if_watcher = Some(if_watch::tokio::IfWatcher::new()?);
            pending_event = None;
        } else {
            if_watcher = None;
            listening_addresses.insert(socket_addr.ip());
            let ma = socketaddr_to_multiaddr(&socket_addr, cert_hashes);
            pending_event = Some(TransportEvent::NewAddress {
                listener_id,
                listen_addr: ma,
            })
        }


        let endpoint_c = Arc::clone(&endpoint);
        let accept = async move { endpoint_c.accept().await }.boxed();

        Ok(
            Listener {
                listener_id,
                endpoint,
                accept,
                handshake_timeout,
                if_watcher,
                is_closed: false,
                pending_event,
                close_listener_waker: None,
                listening_addresses,
                keypair: keypair.clone(),
                cert_hashes: cert_hashes.clone(),
            }
        )
    }

    fn socket_addr(&self) -> SocketAddr {
        self.endpoint
            .local_addr()
            .expect("Cannot fail because the socket is bound")
    }

    fn noise_config(&self) -> libp2p_noise::Config {
        let mut res = libp2p_noise::Config::new(&self.keypair).expect("Getting a noise config");
        // let mut set = HashSet::with_capacity(self.cert_hashes.len());
        let set = self.cert_hashes.iter().cloned().collect::<HashSet<_>>();
        let res = res.with_webtransport_certhashes(set);

        res
    }


    /* todo poll_if_addr
    /// Poll for a next If Event.
    fn poll_if_addr(&mut self, cx: &mut Context<'_>) -> Poll<<Self as Stream>::Item> {
        let endpoint_addr = self.socket_addr();
        let Some(if_watcher) = self.if_watcher.as_mut() else {
            return Poll::Pending;
        };
        loop {
            match ready!(P::poll_if_event(if_watcher, cx)) {
                Ok(IfEvent::Up(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listenaddr(&endpoint_addr, inet.addr(), self.version)
                    {
                        tracing::debug!(
                            address=%listen_addr,
                            "New listen address"
                        );
                        self.listening_addresses.insert(inet.addr());
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listenaddr(&endpoint_addr, inet.addr(), self.version)
                    {
                        tracing::debug!(
                            address=%listen_addr,
                            "Expired listen address"
                        );
                        self.listening_addresses.remove(&inet.addr());
                        return Poll::Ready(TransportEvent::AddressExpired {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Err(err) => {
                    return Poll::Ready(TransportEvent::ListenerError {
                        listener_id: self.listener_id,
                        error: err.into(),
                    })
                }
            }
        }
    }*/
}

impl Stream for Listener {
    type Item = TransportEvent<<GenTransport as Transport>::ListenerUpgrade, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.pending_event.take() {
                return Poll::Ready(Some(event));
            }
            if self.is_closed {
                return Poll::Ready(None);
            }
            /* todo poll_if_addr
            if let Poll::Ready(event) = self.poll_if_addr(cx) {
                return Poll::Ready(Some(event));
            }*/

            match self.accept.poll_unpin(cx) {
                Poll::Ready(incoming_session) => {
                    let endpoint = self.endpoint.clone();
                    self.accept = async move { endpoint.accept().await }.boxed();

                    /*let connecting = match session_request.accept() {
                        Ok(connecting) => connecting,
                        Err(error) => {
                            return Poll::Ready(Some(TransportEvent::ListenerError {
                                listener_id: self.listener_id,
                                error: Error::Connection(crate::ConnectionError(error)),
                            }))
                        }
                    };*/

                    let local_addr = socketaddr_to_multiaddr(&self.socket_addr(), &self.cert_hashes);
                    let remote_addr = incoming_session.remote_address();
                    //todo нафига к удаленному адресу добавлять хэши сертификатов?
                    let send_back_addr = socketaddr_to_multiaddr(&remote_addr, &self.cert_hashes);
                    let noise = self.noise_config();

                    let event = TransportEvent::Incoming {
                        upgrade: Connecting::new(incoming_session, noise, self.handshake_timeout),
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    };
                    return Poll::Ready(Some(event));
                }
                Poll::Pending => {}
            };

            self.close_listener_waker = Some(cx.waker().clone());

            return Poll::Pending;
        }
    }
}

/// Turns an IP address and port into the corresponding WebTransport multiaddr.
fn socketaddr_to_multiaddr(socket_addr: &SocketAddr, hashes: &Vec<Multihash<64>>) -> Multiaddr {
    let mut vec = hashes.clone();
    let mut res = Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::QuicV1)
        .with(Protocol::WebTransport)
        .with(Protocol::Certhash(
            vec.pop().expect("Gets the last element"),
        ));
    if !vec.is_empty() {
        res = res.with(Protocol::Certhash(
            vec.pop().expect("Gets the last element"),
        ));
    };

    res
}

/// Tries to turn a Webtransport multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
fn multiaddr_to_socketaddr(
    addr: &Multiaddr,
) -> Option<(SocketAddr, Option<PeerId>)> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    if proto3 != Protocol::QuicV1 {
        tracing::error!("Cannot listen on a non QUIC address {addr}");
        return None;
    }

    let mut peer_id = None;
    let mut is_webtransport = false;
    for proto in iter {
        match proto {
            Protocol::P2p(id) => {
                peer_id = Some(id);
            }
            Protocol::WebTransport => {
                is_webtransport = true;
            }
            Protocol::Certhash(_) => {
                tracing::error!("Cannot listen on a specific certhash for WebTransport address {addr}");
                return None;
            }
            _ => return None,
        }
    }

    if !is_webtransport {
        tracing::error!("Listening address {addr} should be followed by `/webtransport`");
        return None;
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Udp(port)) => Some((
            SocketAddr::new(ip.into(), port),
            peer_id,
        )),
        (Protocol::Ip6(ip), Protocol::Udp(port)) => Some((
            SocketAddr::new(ip.into(), port),
            peer_id,
        )),
        _ => None,
    }
}

#[cfg(test)]
// #[cfg(any(feature = "async-std", feature = "tokio"))]
mod test {
    #[test]
    fn test() {
        println!("hi!")
    }
}