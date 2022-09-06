// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use futures::{
    future,
    future::BoxFuture,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    prelude::*,
    ready,
    stream::SelectAll,
    stream::Stream,
    TryFutureExt,
};
use if_watch::{IfEvent, IfWatcher};
use libp2p_core::{
    identity,
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
    InboundUpgrade, OutboundUpgrade, PeerId, Transport, UpgradeInfo,
};
use libp2p_noise::{Keypair, NoiseConfig, NoiseError, RemoteIdentity, X25519Spec};
use log::{debug, trace};
use tokio_crate::net::UdpSocket;
use webrtc::ice::udp_mux::UDPMux;
use webrtc::peer_connection::certificate::RTCCertificate;
use webrtc::peer_connection::configuration::RTCConfiguration;

use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::{
    connection::Connection,
    connection::PollDataChannel,
    error::Error,
    fingerprint::Fingerprint,
    udp_mux::{UDPMuxEvent, UDPMuxNewAddr},
    webrtc_connection::WebRTCConnection,
};

/// A WebRTC transport with direct p2p communication (without a STUN server).
pub struct WebRTCTransport {
    /// The config which holds this peer's certificate(s).
    config: WebRTCConfiguration,
    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
    /// All the active listeners.
    listeners: SelectAll<WebRTCListenStream>,
}

impl WebRTCTransport {
    /// Creates a new WebRTC transport.
    pub fn new(certificate: RTCCertificate, id_keys: identity::Keypair) -> Self {
        Self {
            config: WebRTCConfiguration::new(certificate),
            id_keys,
            listeners: SelectAll::new(),
        }
    }

    fn do_listen(
        &self,
        listener_id: ListenerId,
        addr: Multiaddr,
    ) -> Result<WebRTCListenStream, TransportError<Error>> {
        let sock_addr =
            multiaddr_to_socketaddr(&addr).ok_or(TransportError::MultiaddrNotSupported(addr))?;

        // XXX: `UdpSocket::bind` is async, so use a std socket and convert
        let std_sock = std::net::UdpSocket::bind(sock_addr)
            .map_err(Error::IoError)
            .map_err(TransportError::Other)?;
        std_sock
            .set_nonblocking(true)
            .map_err(Error::IoError)
            .map_err(TransportError::Other)?;
        let socket = UdpSocket::from_std(std_sock)
            .map_err(Error::IoError)
            .map_err(TransportError::Other)?;

        let listen_addr = socket
            .local_addr()
            .map_err(Error::IoError)
            .map_err(TransportError::Other)?;
        debug!("listening on {}", listen_addr);

        let udp_mux = UDPMuxNewAddr::new(socket);

        Ok(WebRTCListenStream::new(
            listener_id,
            listen_addr,
            self.config.clone(),
            udp_mux,
            self.id_keys.clone(),
            IfWatcher::new()
                .map_err(Error::IoError)
                .map_err(TransportError::Other)?,
        ))
    }
}

impl Transport for WebRTCTransport {
    type Output = (PeerId, Connection);
    type Error = Error;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let id = ListenerId::new();
        let listener = self.do_listen(id, addr)?;
        self.listeners.push(listener);
        Ok(id)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(listener) = self.listeners.iter_mut().find(|l| l.listener_id == id) {
            listener.close(Ok(()));
            true
        } else {
            false
        }
    }

    /// Poll all listeners.
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        match self.listeners.poll_next_unpin(cx) {
            Poll::Ready(Some(ev)) => Poll::Ready(ev),
            _ => Poll::Pending,
        }
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let sock_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if sock_addr.port() == 0 || sock_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let remote = addr.clone(); // used for logging
        trace!("dialing addr={}", remote);

        let config = self.config.clone();
        let our_fingerprint = self.config.fingerprint_of_first_certificate();
        let id_keys = self.id_keys.clone();

        let first_listener = self
            .listeners
            .iter()
            .next()
            .ok_or(TransportError::Other(Error::NoListeners))?;
        let udp_mux = first_listener.udp_mux.udp_mux_handle();

        // [`Transport::dial`] should do no work unless the returned [`Future`] is polled. Thus
        // do the `set_remote_description` call within the [`Future`].
        Ok(async move {
            let remote_fingerprint = fingerprint_from_addr(&addr)
                .ok_or_else(|| Error::InvalidMultiaddr(addr.clone()))?;

            let conn = WebRTCConnection::connect(
                sock_addr,
                config.into_inner(),
                udp_mux,
                &remote_fingerprint,
            )
            .await?;

            // Open a data channel to do Noise on top and verify the remote.
            let data_channel = conn.create_initial_upgrade_data_channel().await?;

            trace!("noise handshake with addr={}", remote);
            let peer_id = perform_noise_handshake_outbound(
                id_keys,
                PollDataChannel::new(data_channel.clone()),
                our_fingerprint,
                remote_fingerprint,
            )
            .await?;

            trace!("verifying peer's identity addr={}", remote);
            let peer_id_from_addr = PeerId::try_from_multiaddr(&addr);
            if peer_id_from_addr.is_none() || peer_id_from_addr.unwrap() != peer_id {
                return Err(Error::InvalidPeerID {
                    expected: peer_id_from_addr,
                    got: peer_id,
                });
            }

            // Close the initial data channel after noise handshake is done.
            data_channel
                .close()
                .await
                .map_err(|e| Error::WebRTC(webrtc::Error::Data(e)))?;

            let mut c = Connection::new(conn.into_inner()).await;
            // TODO: default buffer size is too small to fit some messages. Possibly remove once
            // https://github.com/webrtc-rs/sctp/issues/28 is fixed.
            c.set_data_channels_read_buf_capacity(8192 * 10);
            Ok((peer_id, c))
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        // TODO: As the listener of a WebRTC hole punch, we need to send a random UDP packet to the
        // `addr`. See DCUtR specification below.
        //
        // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
        self.dial(addr)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        libp2p_core::address_translation(server, observed)
    }
}

/// A stream of incoming connections on one or more interfaces.
pub struct WebRTCListenStream {
    /// The ID of this listener.
    listener_id: ListenerId,

    /// The socket address that the listening socket is bound to,
    /// which may be a "wildcard address" like `INADDR_ANY` or `IN6ADDR_ANY`
    /// when listening on all interfaces for IPv4 respectively IPv6 connections.
    listen_addr: SocketAddr,

    /// The config which holds this peer's certificate(s).
    config: WebRTCConfiguration,

    /// The UDP muxer that manages all ICE connections.
    udp_mux: UDPMuxNewAddr,

    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,

    /// Set to `Some` if this listener should close.
    ///
    /// Optionally contains a [`TransportEvent::ListenerClosed`] that should be
    /// reported before the listener's stream is terminated.
    report_closed: Option<Option<<Self as Stream>::Item>>,

    /// Watcher for network interface changes.
    /// Reports [`IfEvent`]s for new / deleted ip-addresses when interfaces
    /// become or stop being available.
    ///
    /// `None` if the socket is only listening on a single interface.
    if_watcher: IfWatcher,
}

impl WebRTCListenStream {
    /// Constructs a `WebRTCListenStream` for incoming connections.
    fn new(
        listener_id: ListenerId,
        listen_addr: SocketAddr,
        config: WebRTCConfiguration,
        udp_mux: UDPMuxNewAddr,
        id_keys: identity::Keypair,
        if_watcher: IfWatcher,
    ) -> Self {
        WebRTCListenStream {
            listener_id,
            listen_addr,
            config,
            udp_mux,
            id_keys,
            report_closed: None,
            if_watcher,
        }
    }

    /// Report the listener as closed in a [`TransportEvent::ListenerClosed`] and
    /// terminate the stream.
    fn close(&mut self, reason: Result<(), Error>) {
        match self.report_closed {
            Some(_) => debug!("Listener was already closed."),
            None => {
                // Report the listener event as closed.
                let _ = self
                    .report_closed
                    .insert(Some(TransportEvent::ListenerClosed {
                        listener_id: self.listener_id,
                        reason,
                    }));
            }
        }
    }

    fn poll_if_watcher(&mut self, cx: &mut Context<'_>) -> Poll<<Self as Stream>::Item> {
        while let Poll::Ready(event) = self.if_watcher.poll_if_event(cx) {
            match event {
                Ok(IfEvent::Up(inet)) => {
                    let ip = inet.addr();
                    if self.listen_addr.is_ipv4() == ip.is_ipv4()
                        || self.listen_addr.is_ipv6() == ip.is_ipv6()
                    {
                        let socket_addr = SocketAddr::new(ip, self.listen_addr.port());
                        let ma = socketaddr_to_multiaddr(&socket_addr);
                        log::debug!("New listen address: {}", ma);
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr: ma,
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    let ip = inet.addr();
                    if self.listen_addr.is_ipv4() == ip.is_ipv4()
                        || self.listen_addr.is_ipv6() == ip.is_ipv6()
                    {
                        let socket_addr = SocketAddr::new(ip, self.listen_addr.port());
                        let ma = socketaddr_to_multiaddr(&socket_addr);
                        log::debug!("Expired listen address: {}", ma);
                        return Poll::Ready(TransportEvent::AddressExpired {
                            listener_id: self.listener_id,
                            listen_addr: ma,
                        });
                    }
                }
                Err(err) => {
                    log::debug!("Error when polling network interfaces {}", err);
                    return Poll::Ready(TransportEvent::ListenerError {
                        listener_id: self.listener_id,
                        error: err.into(),
                    });
                }
            }
        }

        Poll::Pending
    }
}

impl Stream for WebRTCListenStream {
    type Item = TransportEvent<<WebRTCTransport as Transport>::ListenerUpgrade, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(closed) = self.report_closed.as_mut() {
                // Listener was closed.
                // Report the transport event if there is one. On the next iteration, return
                // `Poll::Ready(None)` to terminate the stream.
                return Poll::Ready(closed.take());
            }

            if let Poll::Ready(event) = self.poll_if_watcher(cx) {
                return Poll::Ready(Some(event));
            }

            // Poll UDP muxer for new addresses or incoming data for streams.
            match ready!(self.udp_mux.poll(cx)) {
                UDPMuxEvent::NewAddr(new_addr) => {
                    let local_addr = socketaddr_to_multiaddr(&self.listen_addr);
                    let send_back_addr = socketaddr_to_multiaddr(&new_addr.addr);
                    let event = TransportEvent::Incoming {
                        upgrade: Box::pin(upgrade(
                            self.udp_mux.udp_mux_handle(),
                            self.config.clone(),
                            new_addr.addr,
                            new_addr.ufrag,
                            self.id_keys.clone(),
                        )) as BoxFuture<'static, _>,
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    };
                    return Poll::Ready(Some(event));
                }
                UDPMuxEvent::Error(e) => {
                    self.close(Err(Error::UDPMuxError(e)));
                }
            }
        }
    }
}

/// A wrapper around [`RTCConfiguration`].
#[derive(Clone)]
pub(crate) struct WebRTCConfiguration {
    inner: RTCConfiguration,
}

impl WebRTCConfiguration {
    /// Creates a new config.
    pub fn new(certificate: RTCCertificate) -> Self {
        Self {
            inner: RTCConfiguration {
                certificates: vec![certificate],
                ..RTCConfiguration::default()
            },
        }
    }

    /// Returns a SHA-256 fingerprint of the first certificate.
    ///
    /// # Panics
    ///
    /// Panics if the config does not contain any certificates.
    pub fn fingerprint_of_first_certificate(&self) -> Fingerprint {
        // safe to unwrap here because we require a certificate during construction.
        let fingerprints = self
            .inner
            .certificates
            .first()
            .expect("at least one certificate")
            .get_fingerprints()
            .expect("get_fingerprints to succeed");
        // TODO:
        //   modify webrtc-rs to return value in upper-hex rather than lower-hex
        //   Fingerprint::from(fingerprints.first().unwrap())
        debug_assert_eq!("sha-256", fingerprints.first().unwrap().algorithm);
        Fingerprint::new_sha256(fingerprints.first().unwrap().value.to_uppercase())
    }

    /// Consumes the `WebRTCConfiguration`, returning its inner configuration.
    pub fn into_inner(self) -> RTCConfiguration {
        self.inner
    }
}

/// Turns an IP address and port into the corresponding WebRTC multiaddr.
pub(crate) fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::WebRTC)
}

/// Extracts a SHA-256 fingerprint from the given address. Returns `None` if the address does not
/// contain one.
fn fingerprint_from_addr(addr: &Multiaddr) -> Option<Fingerprint> {
    let iter = addr.iter();
    for proto in iter {
        match proto {
            // Only support SHA-256 (0x12) for now.
            Protocol::Certhash(f) if f.code() == 0x12 => return Some(Fingerprint::from(f)),
            _ => continue,
        }
    }
    None
}

/// Tries to turn a WebRTC multiaddress into a [`SocketAddr`]. Returns None if the format of the
/// multiaddr is wrong.
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    // Return `None` if protocols other than `p2p` or `certhash` are present.
    for proto in iter {
        match proto {
            Protocol::P2p(_) => {}
            Protocol::Certhash(_) => {}
            _ => return None,
        }
    }

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::WebRTC) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::WebRTC) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        _ => None,
    }
}

async fn perform_noise_handshake_outbound<T>(
    id_keys: identity::Keypair,
    poll_data_channel: T,
    our_fingerprint: Fingerprint,
    remote_fingerprint: Fingerprint,
) -> Result<PeerId, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    let noise = NoiseConfig::xx(dh_keys);
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, mut noise_io) = noise
        .upgrade_outbound(poll_data_channel, info)
        .and_then(|(remote, io)| match remote {
            RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
            _ => future::err(NoiseError::AuthenticationFailed),
        })
        .await?;

    // Exchange TLS certificate fingerprints to prevent MiM attacks.
    debug!(
        "exchanging TLS certificate fingerprints with peer_id={}",
        peer_id
    );
    debug_assert_eq!("sha-256", our_fingerprint.algorithm());
    let n = noise_io
        .write(&our_fingerprint.value().into_bytes())
        .await?;
    noise_io.flush().await?;
    let mut buf = vec![0; n]; // ASSERT: fingerprint's format is the same.
    noise_io.read_exact(buf.as_mut_slice()).await?;
    let fingerprint_from_noise = Fingerprint::new_sha256(
        String::from_utf8(buf).map_err(|_| Error::Noise(NoiseError::AuthenticationFailed))?,
    );
    if fingerprint_from_noise != remote_fingerprint {
        return Err(Error::InvalidFingerprint {
            expected: remote_fingerprint.value(),
            got: fingerprint_from_noise.value(),
        });
    }

    Ok(peer_id)
}

async fn upgrade(
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    config: WebRTCConfiguration,
    socket_addr: SocketAddr,
    ufrag: String,
    id_keys: identity::Keypair,
) -> Result<(PeerId, Connection), Error> {
    trace!("upgrading addr={} (ufrag={})", socket_addr, ufrag);

    let our_fingerprint = config.fingerprint_of_first_certificate();

    let conn = WebRTCConnection::accept(
        socket_addr,
        config.into_inner(),
        udp_mux,
        &our_fingerprint,
        &ufrag,
    )
    .await?;

    // Open a data channel to do Noise on top and verify the remote.
    let data_channel = conn.create_initial_upgrade_data_channel().await?;

    trace!(
        "noise handshake with addr={} (ufrag={})",
        socket_addr,
        ufrag
    );
    let remote_fingerprint = conn.get_remote_fingerprint().await;
    let peer_id = perform_noise_handshake_inbound(
        id_keys,
        PollDataChannel::new(data_channel.clone()),
        our_fingerprint,
        remote_fingerprint,
    )
    .await?;

    // Close the initial data channel after noise handshake is done.
    data_channel
        .close()
        .await
        .map_err(|e| Error::WebRTC(webrtc::Error::Data(e)))?;

    let mut c = Connection::new(conn.into_inner()).await;
    // TODO: default buffer size is too small to fit some messages. Possibly remove once
    // https://github.com/webrtc-rs/sctp/issues/28 is fixed.
    c.set_data_channels_read_buf_capacity(8192 * 10);

    Ok((peer_id, c))
}

async fn perform_noise_handshake_inbound<T>(
    id_keys: identity::Keypair,
    poll_data_channel: T,
    our_fingerprint: Fingerprint,
    remote_fingerprint: Fingerprint,
) -> Result<PeerId, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    let noise = NoiseConfig::xx(dh_keys);
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, mut noise_io) = noise
        .upgrade_inbound(poll_data_channel, info)
        .and_then(|(remote, io)| match remote {
            RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
            _ => future::err(NoiseError::AuthenticationFailed),
        })
        .await?;

    // Exchange TLS certificate fingerprints to prevent MiM attacks.
    debug!(
        "exchanging TLS certificate fingerprints with peer_id={}",
        peer_id
    );

    // 1. Submit SHA-256 fingerprint
    debug_assert_eq!("sha-256", our_fingerprint.algorithm());
    let n = noise_io
        .write(&our_fingerprint.value().into_bytes())
        .await?;
    noise_io.flush().await?;

    // 2. Receive one too and compare it to the fingerprint of the remote DTLS certificate.
    let mut buf = vec![0; n]; // ASSERT: fingerprint's format is the same.
    noise_io.read_exact(buf.as_mut_slice()).await?;
    let fingerprint_from_noise = Fingerprint::new_sha256(
        String::from_utf8(buf).map_err(|_| Error::Noise(NoiseError::AuthenticationFailed))?,
    );
    if fingerprint_from_noise != remote_fingerprint {
        return Err(Error::InvalidFingerprint {
            expected: remote_fingerprint.value(),
            got: fingerprint_from_noise.value(),
        });
    }

    Ok(peer_id)
}

// Tests //////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use futures::future::poll_fn;
    use libp2p_core::{multiaddr::Protocol, Multiaddr};
    use rcgen::KeyPair;
    use tokio_crate as tokio;

    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn multiaddr_to_socketaddr_conversion() {
        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_none()
        );

        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/udp/12345/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some( SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                    12345,
            ) )
        );

        assert!(
            multiaddr_to_socketaddr(
                &"/ip4/127.0.0.1/tcp/12345/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w"
                    .parse::<Multiaddr>()
                    .unwrap()
            ).is_none()
        );

        assert!(multiaddr_to_socketaddr(
            &"/ip4/127.0.0.1/udp/12345/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w/tcp/12345"
                .parse::<Multiaddr>()
                .unwrap()
        )
        .is_none());

        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/255.255.255.255/udp/8080/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                    8080,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/::1/udp/12345/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some( SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                    12345,
            ) )
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/udp/8080/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Some( SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::new(
                            65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
                    )),
                    8080,
            ) )
        );
    }

    #[tokio::test]
    async fn close_listener() {
        let id_keys = identity::Keypair::generate_ed25519();
        let mut transport = {
            let kp = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key pair");
            let cert = RTCCertificate::from_key_pair(kp).expect("certificate");
            WebRTCTransport::new(cert, id_keys)
        };

        assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
            .now_or_never()
            .is_none());

        // Run test twice to check that there is no unexpected behaviour if `QuicTransport.listener`
        // is temporarily empty.
        for _ in 0..2 {
            let listener = transport
                .listen_on("/ip4/0.0.0.0/udp/0/webrtc".parse().unwrap())
                .unwrap();
            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::NewAddress {
                    listener_id,
                    listen_addr,
                } => {
                    assert_eq!(listener_id, listener);
                    assert!(
                        matches!(listen_addr.iter().next(), Some(Protocol::Ip4(a)) if !a.is_unspecified())
                    );
                    assert!(
                        matches!(listen_addr.iter().nth(1), Some(Protocol::Udp(port)) if port != 0)
                    );
                    assert!(matches!(listen_addr.iter().nth(2), Some(Protocol::WebRTC)));
                }
                e => panic!("Unexpected event: {:?}", e),
            }
            assert!(
                transport.remove_listener(listener),
                "Expect listener to exist."
            );
            match poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx)).await {
                TransportEvent::ListenerClosed {
                    listener_id,
                    reason: Ok(()),
                } => {
                    assert_eq!(listener_id, listener);
                }
                e => panic!("Unexpected event: {:?}", e),
            }
            // Poll once again so that the listener has the chance to return `Poll::Ready(None)` and
            // be removed from the list of listeners.
            assert!(poll_fn(|cx| Pin::new(&mut transport).as_mut().poll(cx))
                .now_or_never()
                .is_none());
            assert!(transport.listeners.is_empty());
        }
    }
}
