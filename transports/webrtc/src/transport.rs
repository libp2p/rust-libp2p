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

use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::{
    channel::{mpsc, oneshot},
    future,
    future::BoxFuture,
    prelude::*,
    ready, select, TryFutureExt,
};
use futures_timer::Delay;
use if_watch::{IfEvent, IfWatcher};
use libp2p_core::identity;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    muxing::StreamMuxerBox,
    transport::{Boxed, ListenerEvent, TransportError},
    PeerId, Transport,
};
use libp2p_core::{OutboundUpgrade, UpgradeInfo};
use libp2p_noise::{Keypair, NoiseConfig, NoiseError, RemoteIdentity, X25519Spec};
use log::{debug, trace};
use tinytemplate::TinyTemplate;
use tokio_crate::net::{ToSocketAddrs, UdpSocket};
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

use std::borrow::Cow;
use std::io;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use crate::error::Error;
use crate::sdp;

use crate::connection::Connection;
use crate::connection::PollDataChannel;
use crate::udp_mux::UDPMuxNewAddr;
use crate::udp_mux::UDPMuxParams;
use crate::upgrade;

enum IfWatch {
    Pending(BoxFuture<'static, io::Result<IfWatcher>>),
    Ready(IfWatcher),
}

/// The listening addresses of a [`WebRTCDirectTransport`].
enum InAddr {
    /// The stream accepts connections on a single interface.
    One { out: Option<Multiaddr> },
    /// The stream accepts connections on all interfaces.
    Any { if_watch: IfWatch },
}

/// A WebRTC transport with direct p2p communication (without a STUN server).
#[derive(Clone)]
pub struct WebRTCDirectTransport {
    /// A `RTCConfiguration` which holds this peer's certificate(s).
    config: RTCConfiguration,

    /// The `UDPMux` that manages all ICE connections.
    udp_mux: Arc<dyn UDPMux + Send + Sync>,

    /// The local address of `udp_mux`.
    udp_mux_addr: SocketAddr,

    /// The receiver for new `SocketAddr` connecting to this peer.
    new_addr_rx: Arc<Mutex<mpsc::Receiver<Multiaddr>>>,

    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
}

impl WebRTCDirectTransport {
    /// Create a new WebRTC transport.
    ///
    /// Creates a UDP socket bound to `listen_addr`.
    pub async fn new<A: ToSocketAddrs>(
        certificate: RTCCertificate,
        id_keys: identity::Keypair,
        listen_addr: A,
    ) -> Result<Self, TransportError<Error>> {
        // Bind to `listen_addr` and construct a UDP mux.
        let socket = UdpSocket::bind(listen_addr)
            .map_err(Error::IoError)
            .map_err(TransportError::Other)
            .await?;
        // Sender and receiver for new addresses
        let (new_addr_tx, new_addr_rx) = mpsc::channel(1);
        let udp_mux_addr = socket
            .local_addr()
            .map_err(Error::IoError)
            .map_err(TransportError::Other)?;
        let udp_mux = UDPMuxNewAddr::new(UDPMuxParams::new(socket), new_addr_tx);

        Ok(Self {
            config: RTCConfiguration {
                certificates: vec![certificate],
                ..RTCConfiguration::default()
            },
            udp_mux,
            udp_mux_addr,
            new_addr_rx: Arc::new(Mutex::new(new_addr_rx)),
            id_keys,
        })
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
}

impl Transport for WebRTCDirectTransport {
    type Output = (PeerId, Connection);
    type Error = Error;
    type Listener = WebRTCListenStream;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        debug!("listening on {} (ignoring {})", self.udp_mux_addr, addr);
        Ok(WebRTCListenStream::new(
            self.udp_mux_addr,
            self.config.clone(),
            self.udp_mux.clone(),
            self.new_addr_rx.clone(),
            self.id_keys.clone(),
        ))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        Ok(Box::pin(self.do_dial(addr)))
    }

    fn dial_as_listener(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // XXX: anything to do here?
        self.dial(addr)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        // XXX: anything to do here?
        libp2p_core::address_translation(server, observed)
    }
}

/// A stream of incoming connections on one or more interfaces.
pub struct WebRTCListenStream {
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
    /// How long to sleep after a (non-fatal) error while trying
    /// to accept a new connection.
    sleep_on_error: Duration,
    /// The current pause, if any.
    pause: Option<Delay>,

    /// A `RTCConfiguration` which holds this peer's certificate(s).
    config: RTCConfiguration,
    /// The `UDPMux` that manages all ICE connections.
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    /// The receiver for new `SocketAddr` connecting to this peer.
    new_addr_rx: Arc<Mutex<mpsc::Receiver<Multiaddr>>>,
    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
}

impl WebRTCListenStream {
    /// Constructs a `WebRTCListenStream` for incoming connections around
    /// the given `TcpListener`.
    fn new(
        listen_addr: SocketAddr,
        config: RTCConfiguration,
        udp_mux: Arc<dyn UDPMux + Send + Sync>,
        new_addr_rx: Arc<Mutex<mpsc::Receiver<Multiaddr>>>,
        id_keys: identity::Keypair,
    ) -> Self {
        // Check whether the listening IP is set or not.
        let in_addr = if match &listen_addr {
            SocketAddr::V4(a) => a.ip().is_unspecified(),
            SocketAddr::V6(a) => a.ip().is_unspecified(),
        } {
            // The `addrs` are populated via `if_watch` when the
            // `WebRTCDirectTransport` is polled.
            InAddr::Any {
                if_watch: IfWatch::Pending(IfWatcher::new().boxed()),
            }
        } else {
            InAddr::One {
                out: Some(ip_to_multiaddr(listen_addr.ip(), listen_addr.port())),
            }
        };

        WebRTCListenStream {
            listen_addr,
            in_addr,
            pause: None,
            sleep_on_error: Duration::from_millis(100),
            config,
            udp_mux,
            new_addr_rx,
            id_keys,
        }
    }
}

impl Stream for WebRTCListenStream {
    type Item = Result<
        ListenerEvent<BoxFuture<'static, Result<(PeerId, Connection), Error>>, Error>,
        Error,
    >;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);

        loop {
            match &mut me.in_addr {
                InAddr::Any { if_watch } => match if_watch {
                    // If we listen on all interfaces, wait for `if-watch` to be ready.
                    IfWatch::Pending(f) => match ready!(Pin::new(f).poll(cx)) {
                        Ok(w) => {
                            *if_watch = IfWatch::Ready(w);
                            continue;
                        },
                        Err(err) => {
                            debug! {
                                "Failed to begin observing interfaces: {:?}. Scheduling retry.",
                                err
                            };
                            *if_watch = IfWatch::Pending(IfWatcher::new().boxed());
                            me.pause = Some(Delay::new(me.sleep_on_error));
                            return Poll::Ready(Some(Ok(ListenerEvent::Error(Error::IoError(
                                err,
                            )))));
                        },
                    },
                    // Consume all events for up/down interface changes.
                    IfWatch::Ready(watch) => {
                        while let Poll::Ready(ev) = watch.poll_unpin(cx) {
                            match ev {
                                Ok(IfEvent::Up(inet)) => {
                                    let ip = inet.addr();
                                    if me.listen_addr.is_ipv4() == ip.is_ipv4()
                                        || me.listen_addr.is_ipv6() == ip.is_ipv6()
                                    {
                                        let ma = ip_to_multiaddr(ip, me.listen_addr.port());
                                        debug!("New listen address: {}", ma);
                                        return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(
                                            ma,
                                        ))));
                                    }
                                },
                                Ok(IfEvent::Down(inet)) => {
                                    let ip = inet.addr();
                                    if me.listen_addr.is_ipv4() == ip.is_ipv4()
                                        || me.listen_addr.is_ipv6() == ip.is_ipv6()
                                    {
                                        let ma = ip_to_multiaddr(ip, me.listen_addr.port());
                                        debug!("Expired listen address: {}", ma);
                                        return Poll::Ready(Some(Ok(
                                            ListenerEvent::AddressExpired(ma),
                                        )));
                                    }
                                },
                                Err(err) => {
                                    debug! {
                                        "Failure polling interfaces: {:?}. Scheduling retry.",
                                        err
                                    };
                                    me.pause = Some(Delay::new(me.sleep_on_error));
                                    return Poll::Ready(Some(Ok(ListenerEvent::Error(
                                        Error::IoError(err),
                                    ))));
                                },
                            }
                        }
                    },
                },
                // If the listener is bound to a single interface, make sure the address reported
                // once.
                InAddr::One { out } => {
                    if let Some(multiaddr) = out.take() {
                        return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(multiaddr))));
                    }
                },
            }

            if let Some(mut pause) = me.pause.take() {
                match Pin::new(&mut pause).poll(cx) {
                    Poll::Ready(_) => {},
                    Poll::Pending => {
                        me.pause = Some(pause);
                        return Poll::Pending;
                    },
                }
            }

            // Safe to unwrap here since this is the only place `new_addr_rx` is locked.
            return match Pin::new(&mut *me.new_addr_rx.lock().unwrap()).poll_next(cx) {
                Poll::Ready(Some(addr)) => Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                    local_addr: ip_to_multiaddr(me.listen_addr.ip(), me.listen_addr.port()),
                    remote_addr: addr.clone(),
                    upgrade: Box::pin(upgrade::webrtc(
                        me.udp_mux.clone(),
                        me.config.clone(),
                        addr,
                        me.id_keys.clone(),
                    )) as BoxFuture<'static, _>,
                }))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            };
        }
    }
}

impl WebRTCDirectTransport {
    async fn do_dial(self, addr: Multiaddr) -> Result<(PeerId, Connection), Error> {
        let socket_addr =
            multiaddr_to_socketaddr(&addr).ok_or_else(|| Error::InvalidMultiaddr(addr.clone()))?;
        if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
            return Err(Error::InvalidMultiaddr(addr.clone()));
        }

        let config = self.config.clone();

        let remote = addr.clone(); // used for logging
        trace!("dialing address: {:?}", remote);

        let our_fingerprint = self.cert_fingerprint();
        let se = build_setting_engine(self.udp_mux.clone(), &socket_addr, &our_fingerprint);
        let api = APIBuilder::new().with_setting_engine(se).build();

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
            socket_addr,
            &remote_fingerprint,
        );
        debug!("ANSWER: {:?}", server_session_description);
        let sdp = RTCSessionDescription::answer(server_session_description).unwrap();
        // Set the local description and start UDP listeners
        // Note: this will start the gathering of ICE candidates
        peer_connection
            .set_remote_description(sdp)
            .map_err(Error::WebRTC)
            .await?;

        // Create a datachannel with label 'data'
        let data_channel = peer_connection
            .create_data_channel(
                "data",
                Some(RTCDataChannelInit {
                    id: Some(1),
                    ..RTCDataChannelInit::default()
                }),
            )
            .await?;

        let (tx, mut rx) = oneshot::channel::<Arc<DetachedDataChannel>>();

        // Wait until the data channel is opened and detach it.
        crate::connection::register_data_channel_open_handler(data_channel, tx).await;

        // Wait until data channel is opened and ready to use
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
            .into_authentic(&self.id_keys)
            .unwrap();
        let noise = NoiseConfig::xx(dh_keys);
        let info = noise.protocol_info().next().unwrap();
        let (peer_id, mut noise_io) = noise
            .upgrade_outbound(PollDataChannel::new(detached), info)
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
        let fingerprint_from_noise =
            String::from_utf8(buf).map_err(|_| Error::Noise(NoiseError::AuthenticationFailed))?;
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
        // https://github.com/webrtc-rs/sctp/pull/14
        // detached
        //     .close()
        //     .await
        //     .map_err(|e| Error::WebRTC(e.into()))?;

        Ok((peer_id, Connection::new(peer_connection).await))
    }
}

/// Creates a [`Multiaddr`] from the given IP address and port number.
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    Multiaddr::empty().with(ip.into()).with(Protocol::Udp(port))
}

/// Renders a [`TinyTemplate`] description using the provided arguments.
pub(crate) fn render_description(description: &str, addr: SocketAddr, fingerprint: &str) -> String {
    let mut tt = TinyTemplate::new();
    tt.add_template("description", description).unwrap();

    let f = fingerprint.to_owned().replace(':', "");
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
        // Hashing algorithm (SHA-256) is hardcoded for now
        fingerprint: fingerprint.to_owned(),
        // ufrag and pwd are both equal to the fingerprint (minus the `:` delimiter)
        ufrag: f.clone(),
        pwd: f,
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
            Protocol::P2p(_) => {}, // Ignore a `/p2p/...` prefix of possibly outer protocols, if present.
            _ => return None,
        }
    }

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::XWebRTC(_)) => {
            Some(SocketAddr::new(ip.into(), port))
        },
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::XWebRTC(_)) => {
            Some(SocketAddr::new(ip.into(), port))
        },
        _ => None,
    }
}

/// Transforms a byte array fingerprint into a string.
pub(crate) fn fingerprint_to_string(f: &Cow<'_, [u8; 32]>) -> String {
    let values: Vec<String> = f.iter().map(|x| format! {"{:02x}", x}).collect();
    values.join(":")
}

/// Returns a fingerprint of the first certificate.
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
        .expect("fingerprints to succeed");
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

fn fingerprint_from_addr<'a>(addr: &'a Multiaddr) -> Option<Cow<'a, [u8; 32]>> {
    let iter = addr.iter();
    for proto in iter {
        match proto {
            Protocol::XWebRTC(f) => return Some(f),
            _ => continue,
        }
    }
    None
}

// Tests //////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::{multiaddr::Protocol, Multiaddr, Transport};
    use rcgen::KeyPair;
    use std::net::IpAddr;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use tokio_crate as tokio;

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
        let a = "127.0.0.1:0".parse().unwrap();
        connect(a).await;
    }

    #[tokio::test]
    async fn dialer_connects_to_listener_ipv6() {
        let _ = env_logger::builder().is_test(true).try_init();
        let a = "[::1]:0".parse().unwrap();
        connect(a).await;
    }

    async fn connect(listen_addr: SocketAddr) {
        let id_keys = identity::Keypair::generate_ed25519();
        let t1_peer_id = PeerId::from_public_key(&id_keys.public());
        let transport = {
            let kp = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key pair");
            let cert = RTCCertificate::from_key_pair(kp).expect("certificate");
            WebRTCDirectTransport::new(cert, id_keys, listen_addr)
                .await
                .expect("transport")
        };

        let mut listener = transport
            .clone()
            .listen_on(ip_to_multiaddr(listen_addr.ip(), listen_addr.port()))
            .expect("listener");

        let addr = listener
            .try_next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert_ne!(Some(Protocol::Udp(0)), addr.iter().nth(1));

        let inbound = async move {
            let (conn, _addr) = listener
                .try_filter_map(|e| future::ready(Ok(e.into_upgrade())))
                .try_next()
                .await
                .unwrap()
                .unwrap();
            conn.await
        };

        let transport2 = {
            let kp = KeyPair::generate(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key pair");
            let id_keys = identity::Keypair::generate_ed25519();
            let cert = RTCCertificate::from_key_pair(kp).expect("certificate");
            // okay to reuse `listen_addr` since the port is `0` (any).
            WebRTCDirectTransport::new(cert, id_keys, listen_addr)
                .await
                .expect("transport")
        };
        // TODO: make code cleaner wrt ":"
        let f = &transport.cert_fingerprint().replace(':', "");
        let outbound = transport2
            .dial(
                addr.with(Protocol::XWebRTC(hex_to_cow(f)))
                    .with(Protocol::P2p(t1_peer_id.into())),
            )
            .unwrap();

        let (a, b) = futures::join!(inbound, outbound);
        a.and(b).unwrap();
    }
}
