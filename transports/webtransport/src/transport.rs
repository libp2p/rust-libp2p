use std::{
    collections::HashSet,
    fmt,
    future::Pending,
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::{future::BoxFuture, prelude::*, ready, stream::SelectAll};
use if_watch::{tokio::IfWatcher, IfEvent};
use libp2p_core::{
    multiaddr::Protocol,
    transport::{DialOpts, ListenerId, TransportError, TransportEvent},
    Multiaddr, Transport,
};
use libp2p_identity::{Keypair, PeerId};
use socket2::{Domain, Socket, Type};
use wtransport::{
    endpoint::{endpoint_side::Server, Endpoint, SessionRequest},
    error::ConnectionError,
    ServerConfig,
};

use crate::{certificate::CertHash, config::Config, connection::Connection, Connecting, Error};

pub struct GenTransport {
    config: Config,

    listeners: SelectAll<Listener>,
    /// Waker to poll the transport again when a new listener is added.
    waker: Option<Waker>,
}

impl GenTransport {
    pub fn new(config: Config) -> Self {
        GenTransport {
            config,
            listeners: SelectAll::new(),
            waker: None,
        }
    }

    /// Extract the addr, quic version and peer id from the given [`Multiaddr`].
    fn remote_multiaddr_to_socketaddr(
        &self,
        addr: Multiaddr,
        check_unspecified_addr: bool,
    ) -> Result<(SocketAddr, Option<PeerId>), TransportError<<Self as Transport>::Error>> {
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

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        let (socket_addr, _peer_id) = self.remote_multiaddr_to_socketaddr(addr, false)?;
        let socket = create_socket(socket_addr).map_err(Self::Error::from)?;

        let server_tls_config = self.config.server_tls_config();
        let quic_transport_config = self.config.get_quic_transport_config();

        let config = ServerConfig::builder()
            .with_bind_socket(socket.try_clone().unwrap())
            .with_custom_tls_and_transport(server_tls_config, quic_transport_config)
            .build();

        let endpoint =
            wtransport::Endpoint::server(config).map_err(|e| TransportError::Other(e.into()))?;
        let keypair = &self.config.keypair;
        let cert_hashes = self.config.cert_hashes();
        let handshake_timeout = self.config.handshake_timeout;

        tracing::debug!("Listening on {:?}, listenerId {}", &socket, &id);

        let listener = Listener::new(
            id,
            socket,
            endpoint,
            keypair,
            cert_hashes,
            handshake_timeout,
        )?;
        self.listeners.push(listener);

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(listener) = self.listeners.iter_mut().find(|l| l.listener_id == id) {
            // Close the listener, which will eventually finish its stream.
            // `SelectAll` removes streams once they are finished.
            listener.close(Ok(()));

            tracing::debug!("Listener {id} was removed");

            true
        } else {
            false
        }
    }

    fn dial(
        &mut self,
        _addr: Multiaddr,
        _opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::Other(Error::DialOperationIsNotAllowed))
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        if let Poll::Ready(Some(ev)) = self.listeners.poll_next_unpin(cx) {
            return Poll::Ready(ev);
        }

        match &self.waker {
            None => self.waker = Some(cx.waker().clone()),
            Some(waker) => {
                if !waker.will_wake(cx.waker()) {
                    self.waker = Some(cx.waker().clone());
                }
            }
        };

        Poll::Pending
    }
}

/// Listener for incoming connections.
struct Listener {
    /// Id of the listener.
    listener_id: ListenerId,
    /// Endpoint
    endpoint: Arc<Endpoint<Server>>,
    /// Watcher for network interface changes.
    /// None if we are only listening on a single interface.
    if_watcher: Option<IfWatcher>,
    /// A future to poll new incoming connections.
    accept: BoxFuture<'static, Result<SessionRequest, ConnectionError>>,
    /// Timeout for connection establishment on inbound connections.
    handshake_timeout: Duration,
    /// Whether the listener was closed and the stream should terminate.
    is_closed: bool,
    /// Pending event to reported.
    pending_event: Option<<Self as Stream>::Item>,
    /// The stream must be to awaken after it has been closed to deliver the last event.
    close_listener_waker: Option<Waker>,

    keypair: Keypair,

    cert_hashes: Vec<CertHash>,
}

impl Listener {
    fn new(
        listener_id: ListenerId,
        socket: UdpSocket,
        endpoint: Endpoint<Server>,
        keypair: &Keypair,
        cert_hashes: Vec<CertHash>,
        handshake_timeout: Duration,
    ) -> Result<Self, Error> {
        let endpoint = Arc::new(endpoint);
        let c_endpoint = Arc::clone(&endpoint);
        let accept = Self::accept(c_endpoint, listener_id).boxed();

        let if_watcher;
        let pending_event;
        let local_addr = socket.local_addr()?;
        if local_addr.ip().is_unspecified() {
            if_watcher = Some(IfWatcher::new()?);
            pending_event = None;
        } else {
            if_watcher = None;
            let ma = socketaddr_to_multiaddr_with_hashes(&local_addr, &cert_hashes);
            pending_event = Some(TransportEvent::NewAddress {
                listener_id,
                listen_addr: ma,
            })
        }

        Ok(Listener {
            listener_id,
            endpoint,
            if_watcher,
            accept,
            handshake_timeout,
            is_closed: false,
            pending_event,
            close_listener_waker: None,
            keypair: keypair.clone(),
            cert_hashes,
        })
    }

    async fn accept(
        endpoint: Arc<Endpoint<Server>>,
        id: ListenerId,
    ) -> Result<SessionRequest, ConnectionError> {
        let incoming_session = endpoint.accept().await;

        tracing::debug!(
            "Listener {id} got incoming session from {}",
            incoming_session.remote_address()
        );

        incoming_session.await
    }

    /// Report the listener as closed in a [`TransportEvent::ListenerClosed`] and
    /// terminate the stream.
    fn close(&mut self, reason: Result<(), Error>) {
        if self.is_closed {
            return;
        }
        self.endpoint.close(From::from(0u32), &[]);
        self.pending_event = Some(TransportEvent::ListenerClosed {
            listener_id: self.listener_id,
            reason,
        });
        self.is_closed = true;

        // Wake the stream to deliver the last event.
        if let Some(waker) = self.close_listener_waker.take() {
            waker.wake();
        }
    }

    fn socket_addr(&self) -> SocketAddr {
        self.endpoint
            .local_addr()
            .expect("Cannot fail because the socket is bound")
    }

    fn noise_config(&self) -> libp2p_noise::Config {
        let res = libp2p_noise::Config::new(&self.keypair).expect("Getting a noise config");
        let set = self.cert_hashes.iter().cloned().collect::<HashSet<_>>();

        res.with_webtransport_certhashes(set)
    }

    fn poll_if_addr(&mut self, cx: &mut Context<'_>) -> Poll<<Self as Stream>::Item> {
        let endpoint_addr = self.socket_addr();
        let Some(if_watcher) = self.if_watcher.as_mut() else {
            return Poll::Pending;
        };
        loop {
            match ready!(if_watcher.poll_if_event(cx)) {
                Ok(IfEvent::Up(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listen_addr(&endpoint_addr, inet.addr(), &self.cert_hashes)
                    {
                        tracing::debug!(
                            address=%listen_addr,
                            "New listen address"
                        );
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr,
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if let Some(listen_addr) =
                        ip_to_listen_addr(&endpoint_addr, inet.addr(), &self.cert_hashes)
                    {
                        tracing::debug!(
                            address=%listen_addr,
                            "Expired listen address"
                        );
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
    }
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
            if let Poll::Ready(event) = self.poll_if_addr(cx) {
                return Poll::Ready(Some(event));
            }

            match self.accept.poll_unpin(cx) {
                Poll::Ready(Ok(session_request)) => {
                    tracing::debug!(
                        "Listener {} got session request={:?}",
                        &self.listener_id,
                        session_request.path()
                    );

                    let endpoint = Arc::clone(&self.endpoint);
                    self.accept = Self::accept(endpoint, self.listener_id).boxed();
                    let local_addr =
                        socketaddr_to_multiaddr_with_hashes(&self.socket_addr(), &self.cert_hashes);

                    let remote_addr = session_request.remote_address();
                    let send_back_addr = socketaddr_to_multiaddr(&remote_addr);
                    let noise = self.noise_config();

                    let event = TransportEvent::Incoming {
                        upgrade: Connecting::new(session_request, noise, self.handshake_timeout),
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    };
                    return Poll::Ready(Some(event));
                }
                Poll::Ready(Err(connection_error)) => {
                    tracing::error!("Got the error {}", connection_error);
                    self.close(Err(Error::Connection(connection_error)));

                    continue;
                }
                Poll::Pending => {
                    tracing::debug!("Listener {} pending", &self.listener_id);
                }
            };

            match &self.close_listener_waker {
                None => self.close_listener_waker = Some(cx.waker().clone()),
                Some(waker) => {
                    if !waker.will_wake(cx.waker()) {
                        self.close_listener_waker = Some(cx.waker().clone())
                    }
                }
            }

            return Poll::Pending;
        }
    }
}

impl fmt::Debug for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Listener")
            .field("listener_id", &self.listener_id)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("is_closed", &self.is_closed)
            .field("pending_event", &self.pending_event)
            .finish()
    }
}

fn create_socket(socket_addr: SocketAddr) -> io::Result<UdpSocket> {
    let socket = Socket::new(
        Domain::for_address(socket_addr),
        Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    if socket_addr.is_ipv6() {
        socket.set_only_v6(true)?;
    }

    socket.bind(&socket_addr.into())?;

    Ok(socket.into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SocketFamily {
    Ipv4,
    Ipv6,
}

impl SocketFamily {
    fn is_same(a: &IpAddr, b: &IpAddr) -> bool {
        matches!(
            (a, b),
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_))
        )
    }
}

impl From<IpAddr> for SocketFamily {
    fn from(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => SocketFamily::Ipv4,
            IpAddr::V6(_) => SocketFamily::Ipv6,
        }
    }
}

/// Turn an [`IpAddr`] reported by the interface watcher into a
/// listen-address for the endpoint.
///
/// For this, the `ip` is combined with the port that the endpoint
/// is actually bound.
///
/// Returns `None` if the `ip` is not the same socket family as the
/// address that the endpoint is bound to.
fn ip_to_listen_addr(
    endpoint_addr: &SocketAddr,
    ip: IpAddr,
    hashes: &[CertHash],
) -> Option<Multiaddr> {
    // True if either both addresses are Ipv4 or both Ipv6.
    if !SocketFamily::is_same(&endpoint_addr.ip(), &ip) {
        return None;
    }
    let socket_addr = SocketAddr::new(ip, endpoint_addr.port());
    Some(socketaddr_to_multiaddr_with_hashes(&socket_addr, hashes))
}

/// Turns an IP address and port into the corresponding WebTransport multiaddr.
fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::QuicV1)
        .with(Protocol::WebTransport)
}

fn socketaddr_to_multiaddr_with_hashes(socket_addr: &SocketAddr, hashes: &[CertHash]) -> Multiaddr {
    let mut res = socketaddr_to_multiaddr(socket_addr);

    if !hashes.is_empty() {
        let mut vec = hashes.to_owned();
        res = res.with(Protocol::Certhash(
            vec.pop().expect("Gets the last element"),
        ));
        if !vec.is_empty() {
            res = res.with(Protocol::Certhash(
                vec.pop().expect("Gets the last element"),
            ));
        };
    }

    res
}

/// Tries to turn a Webtransport multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<(SocketAddr, Option<PeerId>)> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    if !matches!(proto3, Protocol::QuicV1) {
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
                tracing::error!(
                    "Cannot listen on a specific certhash for WebTransport address {addr}"
                );
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
        (Protocol::Ip4(ip), Protocol::Udp(port)) => {
            Some((SocketAddr::new(ip.into(), port), peer_id))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port)) => {
            Some((SocketAddr::new(ip.into(), port), peer_id))
        }
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use futures::future::poll_fn;
    use time::{ext::NumericalDuration, OffsetDateTime};

    use super::*;
    use crate::certificate::Certificate;

    fn generate_keypair_and_cert() -> (Keypair, Certificate) {
        let keypair = Keypair::generate_ed25519();
        let not_before = OffsetDateTime::now_utc().checked_sub(1.days()).unwrap();
        let cert = Certificate::generate(&keypair, not_before).expect("Generate certificate");

        (keypair, cert)
    }

    #[tokio::test]
    async fn test_close_listener() {
        let (keypair, cert) = generate_keypair_and_cert();
        let config = Config::new(&keypair, cert);
        let mut transport = GenTransport::new(config);

        assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
            .now_or_never()
            .is_none());

        // Run test twice to check that there is no unexpected behaviour if `Transport.listener`
        // is temporarily empty.
        for _ in 0..2 {
            let id = ListenerId::next();
            transport
                .listen_on(
                    id,
                    "/ip4/0.0.0.0/udp/0/quic-v1/webtransport".parse().unwrap(),
                )
                .unwrap();

            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::NewAddress {
                    listener_id,
                    listen_addr,
                } => {
                    assert_eq!(listener_id, id);
                    assert!(
                        matches!(listen_addr.iter().next(), Some(Protocol::Ip4(a)) if !a.is_unspecified())
                    );
                    assert!(
                        matches!(listen_addr.iter().nth(1), Some(Protocol::Udp(port)) if port != 0)
                    );
                    assert!(matches!(listen_addr.iter().nth(2), Some(Protocol::QuicV1)));
                }
                e => panic!("Unexpected event: {e:?}"),
            }
            assert!(transport.remove_listener(id), "Expect listener to exist.");
            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::ListenerClosed {
                    listener_id,
                    reason: Ok(()),
                } => {
                    assert_eq!(listener_id, id);
                }
                e => panic!("Unexpected event: {e:?}"),
            }
            // Poll once again so that the listener has the chance to return `Poll::Ready(None)` and
            // be removed from the list of listeners.
            assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
                .now_or_never()
                .is_none());
            assert!(transport.listeners.is_empty());
        }
    }

    #[test]
    fn socket_to_multiaddr() {
        let (_keypair, cert) = generate_keypair_and_cert();
        let certs = vec![cert.cert_hash()];
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
        let res = socketaddr_to_multiaddr_with_hashes(&addr, &certs);

        assert!(multiaddr_to_socketaddr(&res.to_string().parse::<Multiaddr>().unwrap()).is_none());
    }

    #[test]
    fn multiaddr_to_udp_conversion() {
        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_none()
        );

        assert!(multiaddr_to_socketaddr(
            &"/ip4/127.0.0.1/udp/1234/quic-v1"
                .parse::<Multiaddr>()
                .unwrap()
        )
        .is_none());

        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/12345/quic-v1/webtransport"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345),
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/255.255.255.255/udp/8080/quic-v1/webtransport"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 8080),
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/55148/quic-v1/webtransport/p2p/12D3KooW9xk7Zp1gejwfwNpfm6L9zH5NL4Bx5rm94LRYJJHJuARZ"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some((SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 55148),
                  Some("12D3KooW9xk7Zp1gejwfwNpfm6L9zH5NL4Bx5rm94LRYJJHJuARZ".parse().unwrap())))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/::1/udp/12345/quic-v1/webtransport"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some((
                SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345),
                None
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/udp/8080/quic-v1/webtransport"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some((
                SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::new(
                        65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
                    )),
                    8080,
                ),
                None
            ))
        );
    }
}
