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

use futures::{future::BoxFuture, prelude::*, ready, stream::SelectAll, stream::Stream};
use if_watch::{IfEvent, IfWatcher};
use libp2p_core::{
    identity,
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
    PeerId,
};
use webrtc::peer_connection::certificate::RTCCertificate;
use webrtc::peer_connection::configuration::RTCConfiguration;

use rand::distributions::DistString;
use std::net::IpAddr;
use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    connection::Connection,
    error::Error,
    fingerprint::Fingerprint,
    udp_mux::{UDPMuxEvent, UDPMuxNewAddr},
    upgrade,
};

/// A WebRTC transport with direct p2p communication (without a STUN server).
pub struct Transport {
    /// The config which holds this peer's certificate(s).
    config: Config,
    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
    /// All the active listeners.
    listeners: SelectAll<ListenStream>,
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
}

impl libp2p_core::Transport for Transport {
    type Output = (PeerId, Connection);
    type Error = Error;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let id = ListenerId::new();

        let socket_addr =
            parse_webrtc_listen_addr(&addr).ok_or(TransportError::MultiaddrNotSupported(addr))?;
        let udp_mux = UDPMuxNewAddr::listen_on(socket_addr)
            .map_err(|io| TransportError::Other(Error::Io(io)))?;

        self.listeners.push(ListenStream::new(
            id,
            self.config.clone(),
            udp_mux,
            self.id_keys.clone(),
            IfWatcher::new().map_err(|io| TransportError::Other(Error::Io(io)))?,
        ));

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
        let (sock_addr, server_fingerprint, expected_peer_id) = parse_webrtc_dial_addr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if sock_addr.port() == 0 || sock_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let config = self.config.clone();
        let client_fingerprint = self.config.fingerprint;
        let id_keys = self.id_keys.clone();
        let udp_mux = self
            .listeners
            .iter()
            .next()
            .ok_or(TransportError::Other(Error::NoListeners))?
            .udp_mux
            .udp_mux_handle();

        Ok(async move {
            let (actual_peer_id, connection) = upgrade::outbound(
                sock_addr,
                config.inner,
                udp_mux,
                client_fingerprint,
                server_fingerprint,
                id_keys,
            )
            .await?;

            if actual_peer_id != expected_peer_id {
                return Err(Error::InvalidPeerID {
                    expected: expected_peer_id,
                    got: actual_peer_id,
                });
            }

            Ok((actual_peer_id, connection))
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
struct ListenStream {
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

impl ListenStream {
    /// Constructs a `WebRTCListenStream` for incoming connections.
    fn new(
        listener_id: ListenerId,
        config: Config,
        udp_mux: UDPMuxNewAddr,
        id_keys: identity::Keypair,
        if_watcher: IfWatcher,
    ) -> Self {
        ListenStream {
            listener_id,
            listen_addr: udp_mux.listen_addr(),
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
            Some(_) => log::debug!("Listener was already closed."),
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
                        return Poll::Ready(TransportEvent::NewAddress {
                            listener_id: self.listener_id,
                            listen_addr: self.listen_multi_address(ip),
                        });
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    let ip = inet.addr();
                    if self.listen_addr.is_ipv4() == ip.is_ipv4()
                        || self.listen_addr.is_ipv6() == ip.is_ipv6()
                    {
                        return Poll::Ready(TransportEvent::AddressExpired {
                            listener_id: self.listener_id,
                            listen_addr: self.listen_multi_address(ip),
                        });
                    }
                }
                Err(err) => {
                    return Poll::Ready(TransportEvent::ListenerError {
                        listener_id: self.listener_id,
                        error: Error::Io(err),
                    });
                }
            }
        }

        Poll::Pending
    }

    /// Constructs a [`Multiaddr`] for the given IP address that represents our listen address.
    fn listen_multi_address(&self, ip: IpAddr) -> Multiaddr {
        let socket_addr = SocketAddr::new(ip, self.listen_addr.port());

        socketaddr_to_multiaddr(&socket_addr, Some(self.config.fingerprint))
    }
}

impl Stream for ListenStream {
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
                    let local_addr =
                        socketaddr_to_multiaddr(&self.listen_addr, Some(self.config.fingerprint));
                    let send_back_addr = socketaddr_to_multiaddr(&new_addr.addr, None);

                    let upgrade = upgrade::inbound(
                        new_addr.addr,
                        self.config.inner.clone(),
                        self.udp_mux.udp_mux_handle(),
                        self.config.fingerprint,
                        new_addr.ufrag,
                        self.id_keys.clone(),
                    )
                    .boxed();

                    return Poll::Ready(Some(TransportEvent::Incoming {
                        upgrade,
                        local_addr,
                        send_back_addr,
                        listener_id: self.listener_id,
                    }));
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
}

/// Turns an IP address and port into the corresponding WebRTC multiaddr.
fn socketaddr_to_multiaddr(socket_addr: &SocketAddr, certhash: Option<Fingerprint>) -> Multiaddr {
    let addr = Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::WebRTC);

    if let Some(fp) = certhash {
        return addr.with(Protocol::Certhash(fp.to_multi_hash()));
    }

    addr
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

// Tests //////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::poll_fn;
    use libp2p_core::{multiaddr::Protocol, Transport as _};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use tokio_crate as tokio;

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
