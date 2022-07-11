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

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::connection::Connection;
use crate::connection::PollDataChannel;
use crate::error::Error;
use crate::sdp;
use crate::udp_mux::UDPMuxNewAddr;
use crate::udp_mux::UDPMuxParams;
use crate::upgrade;

enum IfWatch {
    Pending(BoxFuture<'static, io::Result<IfWatcher>>),
    Ready(IfWatcher),
}

/// The listening addresses of a [`WebRTCTransport`].
enum InAddr {
    /// The stream accepts connections on a single interface.
    One { out: Option<Multiaddr> },
    /// The stream accepts connections on all interfaces.
    Any { if_watch: IfWatch },
}

/// A WebRTC transport with direct p2p communication (without a STUN server).
pub struct WebRTCTransport {
    /// A `RTCConfiguration` which holds this peer's certificate(s).
    config: RTCConfiguration,
    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
    /// All the active listeners.
    /// The `WebRTCListenStream` struct contains a stream that we want to be pinned. Since the `VecDeque`
    /// can be resized, the only way is to use a `Pin<Box<>>`.
    listeners: VecDeque<Pin<Box<WebRTCListenStream>>>,
    /// Pending transport events to return from [`WebRTCTransport::poll`].
    pending_events: VecDeque<TransportEvent<<Self as Transport>::ListenerUpgrade, Error>>,
}

impl WebRTCTransport {
    /// Create a new WebRTC transport.
    pub fn new(certificate: RTCCertificate, id_keys: identity::Keypair) -> Self {
        Self {
            config: RTCConfiguration {
                certificates: vec![certificate],
                ..RTCConfiguration::default()
            },
            id_keys,
            listeners: VecDeque::new(),
            pending_events: VecDeque::new(),
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

        // Sender and receiver for new addresses
        let (new_addr_tx, new_addr_rx) = mpsc::channel(1);
        let udp_mux = UDPMuxNewAddr::new(UDPMuxParams::new(socket), new_addr_tx);

        Ok(WebRTCListenStream::new(
            listener_id,
            listen_addr,
            self.config.clone(),
            udp_mux,
            new_addr_rx,
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
        self.listeners.push_back(Box::pin(listener));
        Ok(id)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(index) = self.listeners.iter().position(|l| l.listener_id != id) {
            self.listeners.remove(index);
            self.pending_events
                .push_back(TransportEvent::ListenerClosed {
                    listener_id: id,
                    reason: Ok(()),
                });
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
        // Return pending events from closed listeners.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        // We remove each element from `listeners` one by one and add them back.
        let mut remaining = self.listeners.len();
        while let Some(mut listener) = self.listeners.pop_back() {
            match TryStream::try_poll_next(listener.as_mut(), cx) {
                Poll::Pending => {
                    self.listeners.push_front(listener);
                    remaining -= 1;
                    if remaining == 0 {
                        break;
                    }
                }
                Poll::Ready(Some(Ok(WebRTCListenerEvent::Upgrade {
                    upgrade,
                    local_addr,
                    remote_addr,
                }))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::Incoming {
                        listener_id: id,
                        upgrade,
                        local_addr,
                        send_back_addr: remote_addr,
                    });
                }
                Poll::Ready(Some(Ok(WebRTCListenerEvent::NewAddress(a)))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::NewAddress {
                        listener_id: id,
                        listen_addr: a,
                    });
                }
                Poll::Ready(Some(Ok(WebRTCListenerEvent::AddressExpired(a)))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::AddressExpired {
                        listener_id: id,
                        listen_addr: a,
                    });
                }
                Poll::Ready(Some(Ok(WebRTCListenerEvent::Error(error)))) => {
                    let id = listener.listener_id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(TransportEvent::ListenerError {
                        listener_id: id,
                        error: error.into(),
                    });
                }
                Poll::Ready(None) => {
                    return Poll::Ready(TransportEvent::ListenerClosed {
                        listener_id: listener.listener_id,
                        reason: Ok(()),
                    });
                }
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(TransportEvent::ListenerClosed {
                        listener_id: listener.listener_id,
                        reason: Err(err),
                    });
                }
            }
        }
        Poll::Pending
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.dial_as_listener(addr)
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let config = self.config.clone();
        let our_fingerprint = self.cert_fingerprint();
        let id_keys = self.id_keys.clone();
        let udp_mux = if let Some(l) = self.listeners.back() {
            l.udp_mux.clone()
        } else {
            return Err(TransportError::Other(Error::NoListeners));
        };

        let sock_addr = multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;
        if sock_addr.port() == 0 || sock_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let remote = addr.clone(); // used for logging
        trace!("dialing address: {:?}", remote);

        let se = build_setting_engine(udp_mux.clone(), &sock_addr, &our_fingerprint);
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
                .into_authentic(&id_keys)
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
            // https://github.com/webrtc-rs/sctp/pull/14
            // detached
            //     .close()
            //     .await
            //     .map_err(|e| Error::WebRTC(e.into()))?;

            let mut c = Connection::new(peer_connection).await;
            // XXX: default buffer size is too small to fit some messages. Possibly remove once
            // https://github.com/webrtc-rs/sctp/issues/28 is fixed.
            c.set_data_channels_read_buf_capacity(8192 * 10);
            Ok((peer_id, c))
        }
        .boxed())
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        libp2p_core::address_translation(server, observed)
    }
}

/// Event produced by a [`WebRTCListenStream`].
#[derive(Debug)]
pub enum WebRTCListenerEvent<S> {
    /// The listener is listening on a new additional [`Multiaddr`].
    NewAddress(Multiaddr),
    /// An upgrade, consisting of the upgrade future, the listener address and the remote address.
    Upgrade {
        /// The upgrade.
        upgrade: S,
        /// The local address which produced this upgrade.
        local_addr: Multiaddr,
        /// The remote address which produced this upgrade.
        remote_addr: Multiaddr,
    },
    /// A [`Multiaddr`] is no longer used for listening.
    AddressExpired(Multiaddr),
    /// A non-fatal error has happened on the listener.
    ///
    /// This event should be generated in order to notify the user that something wrong has
    /// happened. The listener, however, continues to run.
    Error(Error),
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
    new_addr_rx: mpsc::Receiver<Multiaddr>,
    /// `Keypair` identifying this peer
    id_keys: identity::Keypair,
}

impl WebRTCListenStream {
    /// Constructs a `WebRTCListenStream` for incoming connections.
    fn new(
        listener_id: ListenerId,
        listen_addr: SocketAddr,
        config: RTCConfiguration,
        udp_mux: Arc<dyn UDPMux + Send + Sync>,
        new_addr_rx: mpsc::Receiver<Multiaddr>,
        id_keys: identity::Keypair,
    ) -> Self {
        // Check whether the listening IP is set or not.
        let in_addr = if match &listen_addr {
            SocketAddr::V4(a) => a.ip().is_unspecified(),
            SocketAddr::V6(a) => a.ip().is_unspecified(),
        } {
            // The `addrs` are populated via `if_watch` when the
            // `WebRTCTransport` is polled.
            InAddr::Any {
                if_watch: IfWatch::Pending(IfWatcher::new().boxed()),
            }
        } else {
            InAddr::One {
                out: Some(ip_to_multiaddr(listen_addr.ip(), listen_addr.port())),
            }
        };

        WebRTCListenStream {
            listener_id,
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
    type Item =
        Result<WebRTCListenerEvent<BoxFuture<'static, Result<(PeerId, Connection), Error>>>, Error>;

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
                        }
                        Err(err) => {
                            debug! {
                                "Failed to begin observing interfaces: {:?}. Scheduling retry.",
                                err
                            };
                            *if_watch = IfWatch::Pending(IfWatcher::new().boxed());
                            me.pause = Some(Delay::new(me.sleep_on_error));
                            return Poll::Ready(Some(Ok(WebRTCListenerEvent::Error(
                                Error::IoError(err),
                            ))));
                        }
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
                                        return Poll::Ready(Some(Ok(
                                            WebRTCListenerEvent::NewAddress(ma),
                                        )));
                                    }
                                }
                                Ok(IfEvent::Down(inet)) => {
                                    let ip = inet.addr();
                                    if me.listen_addr.is_ipv4() == ip.is_ipv4()
                                        || me.listen_addr.is_ipv6() == ip.is_ipv6()
                                    {
                                        let ma = ip_to_multiaddr(ip, me.listen_addr.port());
                                        debug!("Expired listen address: {}", ma);
                                        return Poll::Ready(Some(Ok(
                                            WebRTCListenerEvent::AddressExpired(ma),
                                        )));
                                    }
                                }
                                Err(err) => {
                                    debug! {
                                        "Failure polling interfaces: {:?}. Scheduling retry.",
                                        err
                                    };
                                    me.pause = Some(Delay::new(me.sleep_on_error));
                                    return Poll::Ready(Some(Ok(WebRTCListenerEvent::Error(
                                        Error::IoError(err),
                                    ))));
                                }
                            }
                        }
                    }
                },
                // If the listener is bound to a single interface, make sure the address reported
                // once.
                InAddr::One { out } => {
                    if let Some(multiaddr) = out.take() {
                        return Poll::Ready(Some(Ok(WebRTCListenerEvent::NewAddress(multiaddr))));
                    }
                }
            }

            if let Some(mut pause) = me.pause.take() {
                match Pin::new(&mut pause).poll(cx) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        me.pause = Some(pause);
                        return Poll::Pending;
                    }
                }
            }

            // Safe to unwrap here since this is the only place `new_addr_rx` is locked.
            return match Pin::new(&mut me.new_addr_rx).poll_next(cx) {
                Poll::Ready(Some(addr)) => Poll::Ready(Some(Ok(WebRTCListenerEvent::Upgrade {
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
    use libp2p_core::{multiaddr::Protocol, Multiaddr};
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
                addr.with(Protocol::XWebRTC(hex_to_cow(&f)))
                    .with(Protocol::P2p(t1_peer_id.into())),
            )
            .unwrap();

        let (a, b) = futures::join!(inbound, outbound);
        a.and(b).unwrap();
    }
}
