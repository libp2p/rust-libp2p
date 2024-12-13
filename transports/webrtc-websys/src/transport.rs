use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures::future::FutureExt;
use libp2p_core::{
    multiaddr::Multiaddr,
    muxing::StreamMuxerBox,
    transport::{Boxed, DialOpts, ListenerId, Transport as _, TransportError, TransportEvent},
};
use libp2p_identity::{Keypair, PeerId};

use super::{upgrade, Connection, Error};

/// Config for the [`Transport`].
#[derive(Clone)]
pub struct Config {
    keypair: Keypair,
}

/// A WebTransport [`Transport`](libp2p_core::Transport) that works with `web-sys`.
pub struct Transport {
    config: Config,
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
        self.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed()
    }
}

impl libp2p_core::Transport for Transport {
    type Output = (PeerId, Connection);
    type Error = Error;
    type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

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

    fn dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        if dial_opts.role.is_listener() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        if maybe_local_firefox() {
            return Err(TransportError::Other(
                "Firefox does not support WebRTC over localhost or 127.0.0.1"
                    .to_string()
                    .into(),
            ));
        }

        let (sock_addr, server_fingerprint) = libp2p_webrtc_utils::parse_webrtc_dial_addr(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr.clone()))?;

        if sock_addr.port() == 0 || sock_addr.ip().is_unspecified() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let config = self.config.clone();

        Ok(async move {
            let (peer_id, connection) =
                upgrade::outbound(sock_addr, server_fingerprint, config.keypair.clone()).await?;

            Ok((peer_id, connection))
        }
        .boxed())
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }
}

/// Checks if local Firefox.
///
/// See: `<https://bugzilla.mozilla.org/show_bug.cgi?id=1659672>` for more details
fn maybe_local_firefox() -> bool {
    let window = &web_sys::window().expect("window should be available");
    let ua = match window.navigator().user_agent() {
        Ok(agent) => agent.to_lowercase(),
        Err(_) => return false,
    };

    let hostname = match window
        .document()
        .expect("should be valid document")
        .location()
    {
        Some(location) => match location.hostname() {
            Ok(hostname) => hostname,
            Err(_) => return false,
        },
        None => return false,
    };

    // check if web_sys::Navigator::user_agent() matches any of the following:
    // - firefox
    // - seamonkey
    // - iceape
    // AND hostname is either localhost or  "127.0.0.1"
    (ua.contains("firefox") || ua.contains("seamonkey") || ua.contains("iceape"))
        && (hostname == "localhost" || hostname == "127.0.0.1" || hostname == "[::1]")
}
