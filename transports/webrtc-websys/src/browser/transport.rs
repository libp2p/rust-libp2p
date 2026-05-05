use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use futures::{channel::oneshot, task::AtomicWaker};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    muxing::StreamMuxerBox,
    transport::{Boxed, DialOpts, ListenerId, Transport as _, TransportError, TransportEvent},
};
use libp2p_identity::{Keypair, PeerId};

use crate::{
    Connection, Error,
    browser::{SignalingConfig, behaviour::Behaviour},
};

/// Config for the [`Transport`].
#[derive(Debug, Clone)]
pub struct Config {
    pub keypair: Keypair,
}

/// Connection request from behavior to transport representing
/// a [`PeerId`] over a [`Connection`].
pub struct ConnectionRequest {
    pub peer_id: PeerId,
    pub connection: Connection,
}

/// A WebRTC [`Transport`](libp2p_core::Transport) that facilitates a `RtcPeerConnection` via
/// web-sys.
pub struct Transport {
    #[allow(unused)]
    config: Config,
    /// Pending connection receivers waiting for WebRTC connections to be established
    pending_dials: Arc<Mutex<HashMap<PeerId, oneshot::Sender<Connection>>>>,
    /// Established WebRTC connections ready to be injected into the swarm
    established_connections: Arc<Mutex<VecDeque<ConnectionRequest>>>,
    /// Listeners for incoming WebRTC connections
    listeners: HashMap<ListenerId, Multiaddr>,
    poll_waker: Arc<AtomicWaker>,
}

impl Transport {
    /// Constructs a new [`Transport`] with the given [`Config`] and [`Behaviour`] for Signaling.
    pub fn new(
        config: Config,
        signaling_config: SignalingConfig,
        poll_waker: Arc<AtomicWaker>,
    ) -> (Self, Behaviour) {
        let pending_dials = Arc::new(Mutex::new(HashMap::new()));
        let established_connections = Arc::new(Mutex::new(VecDeque::new()));

        let transport = Self {
            config: config.clone(),
            pending_dials: pending_dials.clone(),
            established_connections: established_connections.clone(),
            listeners: HashMap::new(),
            poll_waker: poll_waker.clone(),
        };

        let behaviour = Behaviour::new(
            signaling_config,
            pending_dials.clone(),
            established_connections.clone(),
            poll_waker,
        );

        (transport, behaviour)
    }

    /// Wraps `Transport` in and makes it ready to be consumed by SwarmBuilder.
    pub fn boxed(self) -> Boxed<(PeerId, StreamMuxerBox)> {
        self.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed()
    }

    /// Checks if a [`Multiaddr`] is a WebRTC address
    fn is_webrtc_addr(addr: &Multiaddr) -> bool {
        addr.iter().any(|p| matches!(p, Protocol::WebRTC))
    }

    /// Extracts peer ID from multiaddr
    fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
        addr.iter().find_map(|proto| {
            if let Protocol::P2p(peer_id) = proto {
                Some(peer_id)
            } else {
                None
            }
        })
    }
}

impl libp2p_core::Transport for Transport {
    type Output = (PeerId, crate::Connection);
    type Error = Error;
    type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        // Check if this is a WebRTC listener address
        if Self::is_webrtc_addr(&addr) {
            self.listeners.insert(id, addr.clone());
            Ok(())
        } else {
            Err(TransportError::MultiaddrNotSupported(addr))
        }
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.listeners.remove(&id).is_some()
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        _opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let has_circuit = addr.iter().any(|p| matches!(p, Protocol::P2pCircuit));
        if has_circuit {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        if !Self::is_webrtc_addr(&addr) {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let peer_id = Self::extract_peer_id(&addr)
            .ok_or_else(|| TransportError::Other(Error::InvalidMultiaddr(addr.to_string())))?;

        // Register the dial intent synchronously so that the behaviour's
        // `poll` can recognise this peer as one we initiated towards before
        // signaling begins. This is what tells the signaling handler to take
        // the initiator role.
        let (tx, rx) = oneshot::channel();
        self.pending_dials
            .lock()
            .expect("pending_dials mutex poisoned")
            .insert(peer_id, tx);

        Ok(Box::pin(async move {
            let connection = rx.await.map_err(|_| {
                Error::Connection("WebRTC connection establishment cancelled".to_string())
            })?;

            Ok((peer_id, connection))
        }))
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        self.poll_waker.register(cx.waker());

        let mut connections = self
            .established_connections
            .lock()
            .expect("established_connections mutex poisoned");

        if let (Some(conn_request), Some((&listener_id, _))) =
            (connections.pop_front(), self.listeners.iter().next())
        {
            let peer_id = conn_request.peer_id;

            let webrtc_addr = format!("/webrtc/p2p/{}", peer_id)
                .parse::<Multiaddr>()
                .expect("valid webrtc address");

            let upgrade = Box::pin(async move { Ok((peer_id, conn_request.connection)) });

            return Poll::Ready(TransportEvent::Incoming {
                listener_id,
                upgrade,
                local_addr: webrtc_addr.clone(),
                send_back_addr: webrtc_addr,
            });
        }

        Poll::Pending
    }
}
