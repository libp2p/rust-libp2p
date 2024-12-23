use std::collections::HashSet;
use std::fmt;
use std::future::Pending;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures::future::BoxFuture;
use futures::{prelude::*, stream::SelectAll};
use wtransport::endpoint::{endpoint_side::Server, Endpoint, IncomingSession};
use wtransport::{config::TlsServerConfig, ServerConfig};

use libp2p_core::transport::{DialOpts, ListenerId, TransportError, TransportEvent};
use libp2p_core::{multiaddr::Protocol, multihash::Multihash, Multiaddr, Transport};
use libp2p_identity::{Keypair, PeerId};

use crate::certificate::CertHash;
use crate::config::Config;
use crate::connection::Connection;
use crate::Connecting;
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
    ) -> Result<(SocketAddr, Option<PeerId>), TransportError<<Self as Transport>::Error>> {
        //todo rewrite: addr.clone() should be avoided.
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
        let config = ServerConfig::builder()
            .with_bind_address(socket_addr)
            .with_custom_tls(self.server_tls_config.clone())
            .build();

        let endpoint =
            wtransport::Endpoint::server(config).map_err(|e| TransportError::Other(e.into()))?;
        let keypair = &self.keypair;
        let cert_hashes = &self.cert_hashes;
        let handshake_timeout = self.handshake_timeout.clone();

        let listener = Listener::new(id, endpoint, keypair, cert_hashes, handshake_timeout)?;
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
        panic!("Dial operation is not supported!")
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
    /// A future to poll new incoming connections.
    accept: BoxFuture<'static, IncomingSession>,
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
        endpoint: Endpoint<Server>,
        keypair: &Keypair,
        cert_hashes: &Vec<CertHash>,
        handshake_timeout: Duration,
    ) -> Result<Self, Error> {
        let endpoint = Arc::new(endpoint);
        let c_endpoint = Arc::clone(&endpoint);
        let accept = async move { c_endpoint.accept().await }.boxed();

        Ok(Listener {
            listener_id,
            endpoint,
            accept,
            handshake_timeout,
            is_closed: false,
            pending_event: None,
            close_listener_waker: None,
            keypair: keypair.clone(),
            cert_hashes: cert_hashes.clone(),
        })
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
        let res = res.with_webtransport_certhashes(set);

        res
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

            match self.accept.poll_unpin(cx) {
                Poll::Ready(incoming_session) => {
                    let endpoint = Arc::clone(&self.endpoint);
                    self.accept = async move { endpoint.accept().await }.boxed();
                    let mut local_addr = socketaddr_to_multiaddr(&self.socket_addr());
                    local_addr = add_hashes(local_addr, &self.cert_hashes);
                    let remote_addr = incoming_session.remote_address();
                    let send_back_addr = socketaddr_to_multiaddr(&remote_addr);
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

/// Turns an IP address and port into the corresponding WebTransport multiaddr.
fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::QuicV1)
        .with(Protocol::WebTransport)
}

fn add_hashes(m_addr: Multiaddr, hashes: &Vec<Multihash<64>>) -> Multiaddr {
    if !hashes.is_empty() {
        let mut vec = hashes.clone();
        let mut res = m_addr.with(Protocol::Certhash(
            vec.pop().expect("Gets the last element"),
        ));
        if !vec.is_empty() {
            res = res.with(Protocol::Certhash(
                vec.pop().expect("Gets the last element"),
            ));
        };

        res
    } else {
        m_addr
    }
}

/// Tries to turn a Webtransport multiaddress into a UDP [`SocketAddr`]. Returns None if the format
/// of the multiaddr is wrong.
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<(SocketAddr, Option<PeerId>)> {
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
    use time::ext::NumericalDuration;
    use time::OffsetDateTime;

    use crate::certificate::Certificate;

    use super::*;

    #[test]
    fn test() {
        let keypair = Keypair::generate_ed25519();
        let not_before = OffsetDateTime::now_utc().checked_sub(1.days()).unwrap();
        let cert = Certificate::generate(&keypair, not_before).expect("Generate certificate");

        let config = Config::new(&keypair, cert);
        let mut transport = GenTransport::new(config);
        // todo write few tests: closing a listener, addresses parsing and so on
    }
}
