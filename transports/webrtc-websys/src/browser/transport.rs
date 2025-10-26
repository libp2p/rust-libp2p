use std::{
    collections::{HashMap, VecDeque}, future::Future, pin::Pin, sync::Arc, task::{Context, Poll}
};

use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    muxing::StreamMuxerBox,
    transport::{timeout::TransportTimeoutError, DialOpts, ListenerId, Transport as _, TransportError, TransportEvent},
};
use libp2p_identity::{Keypair, PeerId};
use futures::{channel::oneshot, future::BoxFuture, lock::Mutex};
use tracing::instrument;
use crate::{
    browser::{behaviour::Behaviour, SignalingConfig}, Connection, Error
};

/// Config for the [`Transport`].
#[derive(Debug, Clone)]
pub struct Config {
    pub keypair: Keypair,
}

/// Connection request from behavior to transport
pub struct ConnectionRequest {
    pub peer_id: PeerId,
    pub connection: Connection,
}


/// A WebTransport [`Transport`](libp2p_core::Transport) that faciliates a WebRTC [`Connection`].
pub struct Transport {
    config: Config,
    /// Pending connection receivers waiting for WebRTC connections to be established
    pending_dials: Arc<Mutex<HashMap<PeerId, futures::channel::oneshot::Sender<crate::Connection>>>>,
    /// Established WebRTC connections ready to be injected into the swarm
    established_connections: Arc<Mutex<VecDeque<ConnectionRequest>>>,
    /// Listeners for incoming WebRTC connections
    listeners: HashMap<ListenerId, Multiaddr>,
    next_listener_id: ListenerId,
}

impl Transport {
    /// Constructs a new [`Transport`] with the given [`Config`] and [`Behaviour`] for Signaling.
    pub fn new(config: Config, signaling_config: SignalingConfig) -> (Self, Behaviour) {
        let pending_dials = Arc::new(Mutex::new(HashMap::new()));
        let established_connections = Arc::new(Mutex::new(VecDeque::new()));

        let transport = Self {
            config: config.clone(),
            pending_dials: pending_dials.clone(),
            established_connections: established_connections.clone(),
            listeners: HashMap::new(),
            next_listener_id: ListenerId::next()
        };

        let behaviour = Behaviour::new(
            signaling_config,
            pending_dials.clone(),
            established_connections.clone(),
        );

        (transport, behaviour)
    }

    /// Wraps `Transport` in [`Boxed`] and makes it ready to be consumed by
    /// SwarmBuilder.
    pub fn boxed(self) -> libp2p_core::transport::Boxed<(PeerId, StreamMuxerBox)> {
        self.map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)))
            .boxed()
    }

    /// Check if address is a WebRTC address
    fn is_webrtc_addr(addr: &Multiaddr) -> bool {
        tracing::info!("Is WebRTC ADDR? {}", addr);
        addr.iter().any(|p| matches!(p, Protocol::WebRTC))
    }

    /// Check if this is a circuit relay address with WebRTC
    fn is_circuit_webrtc_addr(addr: &Multiaddr) -> bool {
        tracing::info!("Detected circuit webrtc addr in transport {}", addr);
        let has_circuit = addr.iter().any(|p| matches!(p, Protocol::P2pCircuit));
        let has_webrtc = addr.iter().any(|p| matches!(p, Protocol::WebRTC));
        has_circuit && has_webrtc
    }

    /// Extract peer ID from multiaddr
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
        tracing::info!("Calling listen_on for addr {}", addr);
        if Self::is_webrtc_addr(&addr) {
            tracing::info!("Adding listener for addresss {}", addr);
            self.listeners.insert(id, addr.clone());
            Ok(())
        } else {
            tracing::error!("Multiaddr not support for listening {}", addr);
            Err(TransportError::MultiaddrNotSupported(addr))
        }

    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        tracing::info!("Removing listener with listeenr id {}", id);
        self.listeners.remove(&id).is_some()
    }

    #[instrument(skip(self))]
    fn dial(
        &mut self,
        addr: Multiaddr,
        _opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        tracing::info!("Dialing multi addr {}", addr);
        if !Self::is_webrtc_addr(&addr) {
            tracing::info!("Invalid multiaddr {}", addr);
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        if Self::is_circuit_webrtc_addr(&addr) {
            tracing::info!("Invalid multiaddr {}", addr);
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let multiaddr_str = addr.clone().to_string();
        let peer_id = Self::extract_peer_id(&addr).ok_or_else(|| TransportError::Other(Error::InvalidMultiaddr(multiaddr_str)))?;

        let pending_dials = self.pending_dials.clone();

        Ok(Box::pin(async move {
            let (tx, rx) = futures::channel::oneshot::channel();

            {
                let mut dials = pending_dials.lock().await;
                dials.insert(peer_id, tx);
            }

            tracing::info!("Waiitng for the connection to be established via sianling for addr {}", addr);
            let connection = rx.await.map_err(|_| {
                Error::Signaling("webRTC connection establishment cancelled".to_string())
            })?;

            Ok((peer_id, connection))
        }))
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        // Check for established inbound connections that need to be injected
        let established_connections = self.established_connections.clone();

        let mut connections = match established_connections.try_lock() {
            Some(connections) => connections,
            None => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
        };

        if let Some(conn_request) = connections.pop_front() {
            // Find a suitable listener ID
            if let Some((&listener_id, listener_addr)) = self.listeners.iter().next() {
                let upgrade = Box::pin(async move {
                    Ok((conn_request.peer_id, conn_request.connection))
                });

                tracing::info!("Sending Poll::Ready for incoming transport event TransportEvent::Incoming");
                return Poll::Ready(TransportEvent::Incoming {
                    listener_id,
                    upgrade,
                    local_addr: listener_addr.clone(),
                    send_back_addr: listener_addr.clone(),
                });
            }
        }

        Poll::Pending
    }
}
