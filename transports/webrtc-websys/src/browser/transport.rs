use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use libp2p_core::{
    multiaddr::Multiaddr,
    muxing::StreamMuxerBox,
    transport::{DialOpts, ListenerId, Transport as _, TransportError, TransportEvent},
};
use libp2p_identity::{Keypair, PeerId};

use crate::{
    browser::{behaviour::Behaviour, SignalingConfig},
    Error,
};

/// Config for the [`Transport`].
#[derive(Debug, Clone)]
pub struct Config {
    pub keypair: Keypair,
}

/// A WebTransport [`Transport`](libp2p_core::Transport) that faciliates a WebRTC [`Connection`].
pub struct Transport {
    config: Config,
}

impl Transport {
    /// Constructs a new [`Transport`] with the given [`Config`] and [`Behaviour`] for Signaling.
    pub fn new(config: Config, signaling_config: SignalingConfig) -> (Self, Behaviour) {
        let transport = Self {
            config: config.clone(),
        };
        let behaviour = Behaviour::new(signaling_config);

        (transport, behaviour)
    }

    /// Wraps `Transport` in [`Boxed`] and makes it ready to be consumed by
    /// SwarmBuilder.
    pub fn boxed(self) -> libp2p_core::transport::Boxed<(PeerId, StreamMuxerBox)> {
        self.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed()
    }
}

impl libp2p_core::Transport for Transport {
    type Output = (PeerId, crate::Connection);
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
        _opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        return Err(TransportError::MultiaddrNotSupported(addr.clone()));
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }
}
