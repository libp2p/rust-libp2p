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
    future::BoxFuture,
    io::{AsyncRead, AsyncWrite},
    prelude::*,
    ready,
    stream::SelectAll,
    stream::Stream,
};
use if_watch::{IfEvent, IfWatcher};
use libp2p_core::{
    identity,
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
    InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo,
};
use libp2p_noise::{Keypair, NoiseConfig, X25519Spec};
use log::{debug, trace};
use multihash::Multihash;
use tokio_crate::net::UdpSocket;
use webrtc::ice::udp_mux::UDPMux;
use webrtc::peer_connection::certificate::RTCCertificate;
use webrtc::peer_connection::configuration::RTCConfiguration;

use rand::distributions::DistString;
use std::net::IpAddr;
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
pub struct Transport {
    /// The config which holds this peer's certificate(s).
    config: Config,
    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
    /// All the active listeners.
    listeners: SelectAll<WebRTCListenStream>,
}

impl Transport {
    /// Creates a new WebRTC transport.
    pub fn new(id_keys: identity::Keypair) -> Self {
        Self {
            config: Config::new(),
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
            parse_webrtc_listen_addr(&addr).ok_or(TransportError::MultiaddrNotSupported(addr))?;

        // XXX: `UdpSocket::bind` is async, so use a std socket and convert
        let std_sock = std::net::UdpSocket::bind(sock_addr)
            .map_err(Error::Io)
            .map_err(TransportError::Other)?;
        std_sock
            .set_nonblocking(true)
            .map_err(Error::Io)
            .map_err(TransportError::Other)?;
        let socket = UdpSocket::from_std(std_sock)
            .map_err(Error::Io)
            .map_err(TransportError::Other)?;

        let listen_addr = socket
            .local_addr()
            .map_err(Error::Io)
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
                .map_err(Error::Io)
                .map_err(TransportError::Other)?,
        ))
    }
}

impl libp2p_core::Transport for Transport {
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
        let (sock_addr, remote_fingerprint, expected_peer_id) = parse_webrtc_dial_addr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if sock_addr.port() == 0 || sock_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let remote = addr.clone(); // used for logging
        trace!("dialing addr={}", remote);

        let config = self.config.clone();
        let our_fingerprint = self.config.fingerprint();
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
            if expected_peer_id != peer_id {
                return Err(Error::InvalidPeerID {
                    expected: expected_peer_id,
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
struct WebRTCListenStream {
    /// The ID of this listener.
    listener_id: ListenerId,

    /// The socket address that the listening socket is bound to,
    /// which may be a "wildcard address" like `INADDR_ANY` or `IN6ADDR_ANY`
    /// when listening on all interfaces for IPv4 respectively IPv6 connections.
    listen_addr: SocketAddr,

    /// The config which holds this peer's certificate(s).
    config: Config,

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
        config: Config,
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
                        let ma = socketaddr_to_multiaddr(&socket_addr).with(Protocol::Certhash(
                            self.config.fingerprint().to_multi_hash(),
                        ));
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
                        let ma = socketaddr_to_multiaddr(&socket_addr).with(Protocol::Certhash(
                            self.config.fingerprint().to_multi_hash(),
                        ));
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
                        error: Error::Io(err),
                    });
                }
            }
        }

        Poll::Pending
    }
}

impl Stream for WebRTCListenStream {
    type Item = TransportEvent<<Transport as libp2p_core::Transport>::ListenerUpgrade, Error>;

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
                    self.close(Err(Error::UDPMux(e)));
                }
            }
        }
    }
}

#[derive(Clone)]
struct Config {
    inner: RTCConfiguration,
    fingerprint: Fingerprint,
}

impl Config {
    fn new() -> Self {
        let mut params = rcgen::CertificateParams::new(vec![
            rand::distributions::Alphanumeric.sample_string(&mut rand::thread_rng(), 16)
        ]);
        params.alg = &rcgen::PKCS_ECDSA_P256_SHA256;
        let certificate = RTCCertificate::from_params(params).expect("default params to work");

        let fingerprints = certificate.get_fingerprints().expect("to never fail"); // TODO: Remove `Result` upstream?

        Self {
            inner: RTCConfiguration {
                certificates: vec![certificate],
                ..RTCConfiguration::default()
            },
            fingerprint: Fingerprint::try_from_rtc_dtls(
                fingerprints.first().expect("at least one certificate"),
            )
            .expect("we specified SHA-256"),
        }
    }

    /// Returns the fingerprint of our certificate.
    fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Consumes the `WebRTCConfiguration`, returning its inner configuration.
    fn into_inner(self) -> RTCConfiguration {
        self.inner
    }
}

/// Turns an IP address and port into the corresponding WebRTC multiaddr.
fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::WebRTC)
}

/// Parse the given [`Multiaddr`] into a [`SocketAddr`] for listening.
fn parse_webrtc_listen_addr(addr: &Multiaddr) -> Option<SocketAddr> {
    let mut iter = addr.iter();

    let ip = match iter.next()? {
        Protocol::Ip4(ip) => IpAddr::from(ip),
        Protocol::Ip6(ip) => IpAddr::from(ip),
        _ => return None,
    };

    let port = iter.next()?;
    let webrtc = iter.next()?;

    let port = match (port, webrtc) {
        (Protocol::Udp(port), Protocol::WebRTC) => port,
        _ => return None,
    };

    if iter.next().is_some() {
        return None;
    }

    Some(SocketAddr::new(ip, port))
}

/// Parse the given [`Multiaddr`] into a [`SocketAddr`] and a [`Fingerprint`] for dialing.
fn parse_webrtc_dial_addr(addr: &Multiaddr) -> Option<(SocketAddr, Fingerprint, PeerId)> {
    let mut iter = addr.iter();

    let ip = match iter.next()? {
        Protocol::Ip4(ip) => IpAddr::from(ip),
        Protocol::Ip6(ip) => IpAddr::from(ip),
        _ => return None,
    };

    let port = iter.next()?;
    let webrtc = iter.next()?;
    let certhash = iter.next()?;
    let p2p = iter.next()?;

    let (port, fingerprint, peer_id) = match (port, webrtc, certhash, p2p) {
        (
            Protocol::Udp(port),
            Protocol::WebRTC,
            Protocol::Certhash(cert_hash),
            Protocol::P2p(peer_hash),
        ) => {
            let fingerprint = Fingerprint::try_from_multihash(cert_hash)?;
            let peer_id = PeerId::from_multihash(peer_hash).ok()?;

            (port, fingerprint, peer_id)
        }
        _ => return None,
    };

    if iter.next().is_some() {
        return None;
    }

    Some((SocketAddr::new(ip, port), fingerprint, peer_id))
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
    let noise =
        NoiseConfig::xx(dh_keys).with_prologue(noise_prologue(our_fingerprint, remote_fingerprint));
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, _noise_io) = noise
        .into_authenticated()
        .upgrade_outbound(poll_data_channel, info)
        .await?;

    Ok(peer_id)
}

async fn upgrade(
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    config: Config,
    socket_addr: SocketAddr,
    ufrag: String,
    id_keys: identity::Keypair,
) -> Result<(PeerId, Connection), Error> {
    trace!("upgrading addr={} (ufrag={})", socket_addr, ufrag);

    let our_fingerprint = config.fingerprint();

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
    let noise =
        NoiseConfig::xx(dh_keys).with_prologue(noise_prologue(our_fingerprint, remote_fingerprint));
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, _noise_io) = noise
        .into_authenticated()
        .upgrade_inbound(poll_data_channel, info)
        .await?;

    Ok(peer_id)
}

fn noise_prologue(our_fingerprint: Fingerprint, remote_fingerprint: Fingerprint) -> Vec<u8> {
    let (a, b): (Multihash, Multihash) = (
        our_fingerprint.to_multi_hash(),
        remote_fingerprint.to_multi_hash(),
    );
    let (a, b) = (a.to_bytes(), b.to_bytes());
    let (first, second) = if a < b { (a, b) } else { (b, a) };
    const PREFIX: &[u8] = b"libp2p-webrtc-noise:";
    let mut out = Vec::with_capacity(PREFIX.len() + first.len() + second.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(&first);
    out.extend_from_slice(&second);
    out
}

// Tests //////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::poll_fn;
    use hex_literal::hex;
    use libp2p_core::{multiaddr::Protocol, Transport as _};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use tokio_crate as tokio;

    #[test]
    fn noise_prologue_tests() {
        let a = Fingerprint::raw(hex!(
            "3e79af40d6059617a0d83b83a52ce73b0c1f37a72c6043ad2969e2351bdca870"
        ));
        let b = Fingerprint::raw(hex!(
            "30fc9f469c207419dfdd0aab5f27a86c973c94e40548db9375cca2e915973b99"
        ));

        let prologue1 = noise_prologue(a, b);
        let prologue2 = noise_prologue(b, a);

        assert_eq!(hex::encode(&prologue1), "6c69627032702d7765627274632d6e6f6973653a122030fc9f469c207419dfdd0aab5f27a86c973c94e40548db9375cca2e915973b9912203e79af40d6059617a0d83b83a52ce73b0c1f37a72c6043ad2969e2351bdca870");
        assert_eq!(
            prologue1, prologue2,
            "order of fingerprints does not matter"
        );
    }

    #[test]
    fn missing_webrtc_protocol() {
        let addr = "/ip4/127.0.0.1/udp/1234".parse().unwrap();

        let maybe_parsed = parse_webrtc_listen_addr(&addr);

        assert!(maybe_parsed.is_none());
    }

    #[test]
    fn parse_valid_address_with_certhash_and_p2p() {
        let addr = "/ip4/127.0.0.1/udp/39901/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w/p2p/12D3KooWNpDk9w6WrEEcdsEH1y47W71S36yFjw4sd3j7omzgCSMS"
            .parse()
            .unwrap();

        let maybe_parsed = parse_webrtc_dial_addr(&addr);

        assert_eq!(
            maybe_parsed,
            Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 39901),
                Fingerprint::raw(hex_literal::hex!(
                    "e2929e4a5548242ed6b512350df8829b1e4f9d50183c5732a07f99d7c4b2b8eb"
                )),
                "12D3KooWNpDk9w6WrEEcdsEH1y47W71S36yFjw4sd3j7omzgCSMS"
                    .parse::<PeerId>()
                    .unwrap()
            ))
        );
    }

    #[test]
    fn tcp_is_invalid_protocol() {
        let addr = "/ip4/127.0.0.1/tcp/12345/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w"
            .parse()
            .unwrap();

        let maybe_parsed = parse_webrtc_listen_addr(&addr);

        assert!(maybe_parsed.is_none());
    }

    #[test]
    fn cannot_follow_other_protocols_after_certhash() {
        let addr = "/ip4/127.0.0.1/udp/12345/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w/tcp/12345"
            .parse()
            .unwrap();

        let maybe_parsed = parse_webrtc_listen_addr(&addr);

        assert!(maybe_parsed.is_none());
    }

    #[test]
    fn parse_ipv6() {
        let addr =
            "/ip6/::1/udp/12345/webrtc/certhash/uEiDikp5KVUgkLta1EjUN-IKbHk-dUBg8VzKgf5nXxLK46w/p2p/12D3KooWNpDk9w6WrEEcdsEH1y47W71S36yFjw4sd3j7omzgCSMS"
                .parse()
                .unwrap();

        let maybe_parsed = parse_webrtc_dial_addr(&addr);

        assert_eq!(
            maybe_parsed,
            Some((
                SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 12345),
                Fingerprint::raw(hex_literal::hex!(
                    "e2929e4a5548242ed6b512350df8829b1e4f9d50183c5732a07f99d7c4b2b8eb"
                )),
                "12D3KooWNpDk9w6WrEEcdsEH1y47W71S36yFjw4sd3j7omzgCSMS"
                    .parse::<PeerId>()
                    .unwrap()
            ))
        );
    }

    #[test]
    fn can_parse_valid_addr_without_certhash() {
        let addr = "/ip6/::1/udp/12345/webrtc".parse().unwrap();

        let maybe_parsed = parse_webrtc_listen_addr(&addr);

        assert_eq!(
            maybe_parsed,
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 12345))
        );
    }

    #[test]
    fn fails_to_parse_if_certhash_present_but_wrong_hash_function() {
        // We only support SHA2-256 for now but this certhash has been encoded with SHA3-256.
        let addr =
            "/ip6/::1/udp/12345/webrtc/certhash/uFiCH_tkkzpAwkoIDbE4I7QtQksFMYs5nQ4MyYrkgCJYi4A"
                .parse()
                .unwrap();

        let maybe_addr = parse_webrtc_listen_addr(&addr);

        assert!(maybe_addr.is_none())
    }

    #[tokio::test]
    async fn close_listener() {
        let id_keys = identity::Keypair::generate_ed25519();
        let mut transport = Transport::new(id_keys);

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
