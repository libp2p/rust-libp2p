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
    channel::oneshot,
    future,
    future::BoxFuture,
    io::{AsyncReadExt, AsyncWriteExt},
    prelude::*,
    select,
    stream::SelectAll,
    stream::Stream,
    TryFutureExt,
};
use futures_timer::Delay;
use if_watch::IfEvent;
use libp2p_core::{
    identity,
    multiaddr::{Multiaddr, Protocol},
    muxing::StreamMuxerBox,
    transport::{Boxed, ListenerId, TransportError, TransportEvent},
    PeerId, Transport,
};
use libp2p_core::{OutboundUpgrade, UpgradeInfo};
use libp2p_noise::{Keypair, NoiseConfig, NoiseError, RemoteIdentity, X25519Spec};
use log::{debug, trace};
use tinytemplate::TinyTemplate;
use tokio_crate::net::UdpSocket;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::peer_connection::certificate::RTCCertificate;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc_data::data_channel::DataChannel as DetachedDataChannel;
use webrtc_ice::network_type::NetworkType;
use webrtc_ice::udp_mux::UDPMux;
use webrtc_ice::udp_network::UDPNetwork;

use std::{
    borrow::Cow,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use crate::{
    connection::Connection,
    connection::PollDataChannel,
    error::Error,
    in_addr::InAddr,
    sdp,
    udp_mux::{UDPMuxEvent, UDPMuxNewAddr, UDPMuxParams},
    upgrade,
};

/// A WebRTC transport with direct p2p communication (without a STUN server).
pub struct WebRTCTransport {
    /// A `RTCConfiguration` which holds this peer's certificate(s).
    config: RTCConfiguration,
    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
    /// All the active listeners.
    listeners: SelectAll<WebRTCListenStream>,
}

impl WebRTCTransport {
    /// Creates a new WebRTC transport.
    pub fn new(certificate: RTCCertificate, id_keys: identity::Keypair) -> Self {
        Self {
            config: RTCConfiguration {
                certificates: vec![certificate],
                ..RTCConfiguration::default()
            },
            id_keys,
            listeners: SelectAll::new(),
        }
    }

    /// Returns the SHA-256 fingerprint of the certificate in lowercase hex string as expressed
    /// utilizing the syntax of 'fingerprint' in <https://tools.ietf.org/html/rfc4572#section-5>.
    pub fn cert_fingerprint(&self) -> String {
        fingerprint_of_first_certificate(&self.config)
    }

    /// Creates a boxed libp2p transport.
    pub fn boxed(self) -> Boxed<(PeerId, StreamMuxerBox)> {
        Transport::map(self, |(peer_id, conn), _| {
            (peer_id, StreamMuxerBox::new(conn))
        })
        .boxed()
    }

    fn do_listen(
        &self,
        listener_id: ListenerId,
        addr: Multiaddr,
    ) -> Result<WebRTCListenStream, TransportError<Error>> {
        let sock_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;

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

        let udp_mux = UDPMuxNewAddr::new(UDPMuxParams::new(socket));

        Ok(WebRTCListenStream::new(
            listener_id,
            listen_addr,
            self.config.clone(),
            udp_mux,
            self.id_keys.clone(),
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
        let config = self.config.clone();
        let our_fingerprint = self.cert_fingerprint();
        let id_keys = self.id_keys.clone();

        let sock_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if sock_addr.port() == 0 || sock_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }
        let remote = addr.clone(); // used for logging
        trace!("dialing address: {:?}", remote);

        let udp_mux = if let Some(l) = self.listeners.iter_mut().next() {
            l.udp_mux.clone()
        } else {
            return Err(TransportError::Other(Error::NoListeners));
        };
        let se = build_setting_engine(udp_mux, &sock_addr, &our_fingerprint);
        let api = APIBuilder::new().with_setting_engine(se).build();

        // [`Transport::dial`] should do no work unless the returned [`Future`] is polled. Thus
        // do the `set_remote_description` call within the [`Future`].
        Ok(async move {
            let peer_connection = api
                .new_peer_connection(config)
                .map_err(Error::WebRTC)
                .await?;

            let offer = peer_connection
                .create_offer(None)
                .map_err(Error::WebRTC)
                .await?;
            debug!("OFFER: {:?}", offer.sdp);
            peer_connection
                .set_local_description(offer)
                .map_err(Error::WebRTC)
                .await?;

            // Set the remote description to the predefined SDP.
            let remote_fingerprint = match fingerprint_from_addr(&addr) {
                Some(f) => fingerprint_to_string(&f),
                None => return Err(Error::InvalidMultiaddr(addr.clone())),
            };
            let server_session_description = render_description(
                sdp::SERVER_SESSION_DESCRIPTION,
                sock_addr,
                &remote_fingerprint,
                &remote_fingerprint.to_owned().replace(':', ""),
            );
            debug!("ANSWER: {:?}", server_session_description);
            let sdp = RTCSessionDescription::answer(server_session_description).unwrap();
            // Set the local description and start UDP listeners
            // Note: this will start the gathering of ICE candidates
            peer_connection
                .set_remote_description(sdp)
                .map_err(Error::WebRTC)
                .await?;

            // Open a data channel to do Noise on top and verify the remote.
            let data_channel = peer_connection
                .create_data_channel(
                    "data",
                    Some(RTCDataChannelInit {
                        id: Some(1), // id MUST match one used during upgrade
                        ..RTCDataChannelInit::default()
                    }),
                )
                .await?;

            let (tx, mut rx) = oneshot::channel::<Arc<DetachedDataChannel>>();

            // Wait until the data channel is opened and detach it.
            crate::connection::register_data_channel_open_handler(data_channel, tx).await;
            let detached = select! {
                res = rx => match res {
                    Ok(detached) => detached,
                    Err(e) => return Err(Error::InternalError(e.to_string())),
                },
                _ = Delay::new(Duration::from_secs(10)).fuse() => return Err(Error::InternalError(
                    "data channel opening took longer than 10 seconds (see logs)".into(),
                ))
            };

            trace!("noise handshake with {}", remote);
            let dh_keys = Keypair::<X25519Spec>::new()
                .into_authentic(&id_keys)
                .unwrap();
            let noise = NoiseConfig::xx(dh_keys);
            let info = noise.protocol_info().next().unwrap();
            let (peer_id, mut noise_io) = noise
                .upgrade_outbound(PollDataChannel::new(detached.clone()), info)
                .and_then(|(remote, io)| match remote {
                    RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
                    _ => future::err(NoiseError::AuthenticationFailed),
                })
                .await
                .map_err(Error::Noise)?;

            // Exchange TLS certificate fingerprints to prevent MiM attacks.
            trace!("exchanging TLS certificate fingerprints with {}", remote);
            let n = noise_io.write(&our_fingerprint.into_bytes()).await?;
            noise_io.flush().await?;
            let mut buf = vec![0; n]; // ASSERT: fingerprint's format is the same.
            noise_io.read_exact(buf.as_mut_slice()).await?;
            let fingerprint_from_noise = String::from_utf8(buf)
                .map_err(|_| Error::Noise(NoiseError::AuthenticationFailed))?;
            if fingerprint_from_noise != remote_fingerprint {
                return Err(Error::InvalidFingerprint {
                    expected: remote_fingerprint,
                    got: fingerprint_from_noise,
                });
            }

            trace!("verifying peer's identity {}", remote);
            let peer_id_from_addr = PeerId::try_from_multiaddr(&addr);
            if peer_id_from_addr.is_none() || peer_id_from_addr.unwrap() != peer_id {
                return Err(Error::InvalidPeerID {
                    expected: peer_id_from_addr,
                    got: peer_id,
                });
            }

            // Close the initial data channel after noise handshake is done.
            detached
                .close()
                .await
                .map_err(|e| Error::WebRTC(webrtc::Error::Data(e)))?;

            let mut c = Connection::new(peer_connection).await;
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

    /// The IP addresses of network interfaces on which the listening socket
    /// is accepting connections.
    ///
    /// If the listen socket listens on all interfaces, these may change over
    /// time as interfaces become available or unavailable.
    in_addr: InAddr,

    /// A `RTCConfiguration` which holds this peer's certificate(s).
    config: RTCConfiguration,

    /// The UDP muxer that manages all ICE connections.
    udp_mux: Arc<UDPMuxNewAddr>,

    /// Future that drives reading from the UDP socket.
    udp_mux_read_fut: Option<BoxFuture<'static, UDPMuxEvent>>,

    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,

    /// Set to `Some` if this [`Listener`] should close.
    /// Optionally contains a [`TransportEvent::ListenerClosed`] that should be
    /// reported before the listener's stream is terminated.
    report_closed: Option<Option<<Self as Stream>::Item>>,
}

impl WebRTCListenStream {
    /// Constructs a `WebRTCListenStream` for incoming connections.
    fn new(
        listener_id: ListenerId,
        listen_addr: SocketAddr,
        config: RTCConfiguration,
        udp_mux: Arc<UDPMuxNewAddr>,
        id_keys: identity::Keypair,
    ) -> Self {
        let in_addr = InAddr::new(listen_addr.ip());

        WebRTCListenStream {
            listener_id,
            listen_addr,
            in_addr,
            config,
            udp_mux,
            udp_mux_read_fut: None,
            id_keys,
            report_closed: None,
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

    /// Poll for a next If Event.
    fn poll_if_addr(&mut self, cx: &mut Context<'_>) -> Option<<Self as Stream>::Item> {
        match self.in_addr.poll_next_unpin(cx) {
            Poll::Ready(mut item) => {
                if let Some(item) = item.take() {
                    // Consume all events for up/down interface changes.
                    match item {
                        Ok(IfEvent::Up(inet)) => {
                            let ip = inet.addr();
                            if self.listen_addr.is_ipv4() == ip.is_ipv4()
                                || self.listen_addr.is_ipv6() == ip.is_ipv6()
                            {
                                let socket_addr = SocketAddr::new(ip, self.listen_addr.port());
                                let ma = socketaddr_to_multiaddr(&socket_addr);
                                debug!("New listen address: {}", ma);
                                Some(TransportEvent::NewAddress {
                                    listener_id: self.listener_id,
                                    listen_addr: ma,
                                })
                            } else {
                                self.poll_if_addr(cx)
                            }
                        }
                        Ok(IfEvent::Down(inet)) => {
                            let ip = inet.addr();
                            if self.listen_addr.is_ipv4() == ip.is_ipv4()
                                || self.listen_addr.is_ipv6() == ip.is_ipv6()
                            {
                                let socket_addr = SocketAddr::new(ip, self.listen_addr.port());
                                let ma = socketaddr_to_multiaddr(&socket_addr);
                                debug!("Expired listen address: {}", ma);
                                Some(TransportEvent::AddressExpired {
                                    listener_id: self.listener_id,
                                    listen_addr: ma,
                                })
                            } else {
                                self.poll_if_addr(cx)
                            }
                        }
                        Err(err) => {
                            debug! {
                                "Failure polling interfaces: {:?}.",
                                err
                            };
                            Some(TransportEvent::ListenerError {
                                listener_id: self.listener_id,
                                error: err.into(),
                            })
                        }
                    }
                } else {
                    self.poll_if_addr(cx)
                }
            }
            Poll::Pending => None,
        }
    }
}

impl Stream for WebRTCListenStream {
    type Item = TransportEvent<<WebRTCTransport as Transport>::ListenerUpgrade, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Some(closed) = self.report_closed.as_mut() {
            // Listener was closed.
            // Report the transport event if there is one. On the next iteration, return
            // `Poll::Ready(None)` to terminate the stream.
            return Poll::Ready(closed.take());
        }
        if let Some(event) = self.poll_if_addr(cx) {
            return Poll::Ready(Some(event));
        }

        let udp_mux = self.udp_mux.clone();
        let udp_mux_read_fut = self
            .udp_mux_read_fut
            .get_or_insert(Box::pin(async move { udp_mux.read_from_conn().await }));
        match udp_mux_read_fut.as_mut().poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(event) => {
                self.udp_mux_read_fut = None;

                match event {
                    UDPMuxEvent::NewAddr(new_addr) => {
                        let local_addr = socketaddr_to_multiaddr(&self.listen_addr);
                        let send_back_addr = socketaddr_to_multiaddr(&new_addr.addr);
                        let event = TransportEvent::Incoming {
                            upgrade: Box::pin(upgrade::webrtc(
                                self.udp_mux.clone(),
                                self.config.clone(),
                                new_addr.addr,
                                new_addr.ufrag,
                                self.id_keys.clone(),
                            )) as BoxFuture<'static, _>,
                            local_addr,
                            send_back_addr,
                            listener_id: self.listener_id,
                        };
                        Poll::Ready(Some(event))
                    }
                    UDPMuxEvent::Error(e) => {
                        self.close(Err(Error::UDPMuxError(e)));
                        return self.poll_next(cx);
                    }
                    _ => return self.poll_next(cx),
                }
            }
        }
    }
}

// TODO: remove
fn hex_to_cow<'a>(s: &str) -> Cow<'a, [u8; 32]> {
    let mut buf = [0; 32];
    hex::decode_to_slice(s, &mut buf).unwrap();
    Cow::Owned(buf)
}

/// Turns an IP address and port into the corresponding WebRTC multiaddr.
pub(crate) fn socketaddr_to_multiaddr(socket_addr: &SocketAddr) -> Multiaddr {
    // TODO: remove
    let f = "ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B";
    Multiaddr::empty()
        .with(socket_addr.ip().into())
        .with(Protocol::Udp(socket_addr.port()))
        .with(Protocol::XWebRTC(hex_to_cow(&f)))
}

/// Renders a [`TinyTemplate`] description using the provided arguments.
pub(crate) fn render_description(
    description: &str,
    addr: SocketAddr,
    fingerprint: &str,
    ufrag: &str,
) -> String {
    let mut tt = TinyTemplate::new();
    tt.add_template("description", description).unwrap();

    let context = sdp::DescriptionContext {
        ip_version: {
            if addr.is_ipv4() {
                sdp::IpVersion::IP4
            } else {
                sdp::IpVersion::IP6
            }
        },
        target_ip: addr.ip(),
        target_port: addr.port(),
        fingerprint: fingerprint.to_owned(),
        // NOTE: ufrag is equal to pwd.
        ufrag: ufrag.to_owned(),
        pwd: ufrag.to_owned(),
    };
    tt.render("description", &context).unwrap()
}

/// Tries to turn a WebRTC multiaddress into a [`SocketAddr`]. Returns None if the format of the
/// multiaddr is wrong.
pub(crate) fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Option<SocketAddr> {
    let mut iter = addr.iter();
    let proto1 = iter.next()?;
    let proto2 = iter.next()?;
    let proto3 = iter.next()?;

    for proto in iter {
        match proto {
            Protocol::P2p(_) => {} // Ignore a `/p2p/...` prefix of possibly outer protocols, if present.
            _ => return None,
        }
    }

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::XWebRTC(_)) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::XWebRTC(_)) => {
            Some(SocketAddr::new(ip.into(), port))
        }
        _ => None,
    }
}

/// Transforms a byte array fingerprint into a string.
pub(crate) fn fingerprint_to_string(f: &Cow<'_, [u8; 32]>) -> String {
    let values: Vec<String> = f.iter().map(|x| format! {"{:02x}", x}).collect();
    values.join(":")
}

/// Returns a SHA-256 fingerprint of the first certificate.
///
/// # Panics
///
/// Panics if the config does not contain any certificates.
pub(crate) fn fingerprint_of_first_certificate(config: &RTCConfiguration) -> String {
    // safe to unwrap here because we require a certificate during construction.
    let fingerprints = config
        .certificates
        .first()
        .expect("at least one certificate")
        .get_fingerprints()
        .expect("get_fingerprints to succeed");
    debug_assert_eq!("sha-256", fingerprints.first().unwrap().algorithm);
    fingerprints.first().unwrap().value.clone()
}

/// Creates a new [`SettingEngine`] and configures it.
pub(crate) fn build_setting_engine(
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    addr: &SocketAddr,
    fingerprint: &str,
) -> SettingEngine {
    let mut se = SettingEngine::default();
    // Set both ICE user and password to fingerprint.
    // It will be checked by remote side when exchanging ICE messages.
    let f = fingerprint.to_owned().replace(':', "");
    se.set_ice_credentials(f.clone(), f);
    se.set_udp_network(UDPNetwork::Muxed(udp_mux.clone()));
    // Allow detaching data channels.
    se.detach_data_channels();
    // Set the desired network type.
    //
    // NOTE: if not set, a [`webrtc_ice::agent::Agent`] might pick a wrong local candidate
    // (e.g. IPv6 `[::1]` while dialing an IPv4 `10.11.12.13`).
    let network_type = if addr.is_ipv4() {
        NetworkType::Udp4
    } else {
        NetworkType::Udp6
    };
    se.set_network_types(vec![network_type]);
    se
}

/// Extracts a SHA-256 fingerprint from the given address. Returns `None` if the address does not
/// contain one.
fn fingerprint_from_addr<'a>(addr: &'a Multiaddr) -> Option<Cow<'a, [u8; 32]>> {
    let iter = addr.iter();
    for proto in iter {
        match proto {
            // TODO: check hash is one of https://datatracker.ietf.org/doc/html/rfc8122#section-5
            Protocol::XWebRTC(f) => return Some(f),
            _ => continue,
        }
    }
    None
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
                &"/ip4/127.0.0.1/udp/12345/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B"
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
                &"/ip4/127.0.0.1/tcp/12345/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B"
                    .parse::<Multiaddr>()
                    .unwrap()
            ).is_none()
        );

        assert!(multiaddr_to_socketaddr(
            &"/ip4/127.0.0.1/udp/12345/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B/tcp/12345"
                .parse::<Multiaddr>()
                .unwrap()
        )
        .is_none());

        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/255.255.255.255/udp/8080/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B"
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
                &"/ip6/::1/udp/12345/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B"
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
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/udp/8080/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B"
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

    fn hex_to_cow<'a>(s: &str) -> Cow<'a, [u8; 32]> {
        let mut buf = [0; 32];
        hex::decode_to_slice(s, &mut buf).unwrap();
        Cow::Owned(buf)
    }

    #[tokio::test]
    async fn dialer_connects_to_listener_ipv4() {
        let _ = env_logger::builder().is_test(true).try_init();
        let a = "/ip4/127.0.0.1/udp/0/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B".parse().unwrap();
        futures::executor::block_on(connect(a));
    }

    #[tokio::test]
    async fn dialer_connects_to_listener_ipv6() {
        let _ = env_logger::builder().is_test(true).try_init();
        let a = "/ip6/::1/udp/0/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B".parse().unwrap();
        futures::executor::block_on(connect(a));
    }

    async fn connect(listen_addr: Multiaddr) {
        let id_keys = identity::Keypair::generate_ed25519();
        let t1_peer_id = PeerId::from_public_key(&id_keys.public());
        let mut transport = {
            let kp = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key pair");
            let cert = RTCCertificate::from_key_pair(kp).expect("certificate");
            WebRTCTransport::new(cert, id_keys).boxed()
        };

        transport.listen_on(listen_addr.clone()).expect("listener");

        let addr = transport
            .next()
            .await
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert_ne!(Some(Protocol::Udp(0)), addr.iter().nth(1));

        let inbound = async move {
            let (conn, _addr) = transport
                .select_next_some()
                .map(|ev| ev.into_incoming())
                .await
                .unwrap();
            conn.await
        };

        let (mut transport2, f) = {
            let kp = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key pair");
            let id_keys = identity::Keypair::generate_ed25519();
            let cert = RTCCertificate::from_key_pair(kp).expect("certificate");
            let t = WebRTCTransport::new(cert, id_keys);
            // TODO: make code cleaner wrt ":"
            let f = t.cert_fingerprint().replace(':', "");
            (t.boxed(), f)
        };

        transport2.listen_on(listen_addr).expect("listener");

        let outbound = transport2
            .dial(
                addr.replace(2, |_| Some(Protocol::XWebRTC(hex_to_cow(&f))))
                    .unwrap()
                    .with(Protocol::P2p(t1_peer_id.into())),
            )
            .unwrap();

        let (a, b) = futures::join!(inbound, outbound);
        a.and(b).unwrap();
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
                .listen_on("/ip4/0.0.0.0/udp/0/x-webrtc/ACD1E533EC271FCDE0275947F4D62A2B2331FF10C9DDE0298EB7B399B4BFF60B".parse().unwrap())
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
                    assert!(
                        matches!(listen_addr.iter().nth(2), Some(Protocol::XWebRTC(f)) if !f.is_empty())
                    );
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
