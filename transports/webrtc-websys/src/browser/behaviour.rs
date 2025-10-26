use std::{
    collections::{HashMap, VecDeque}, sync::Arc, task::Poll, time::Duration
};

use futures::lock::Mutex;
use tracing::info;

use wasm_timer::Instant;

use libp2p_core::{multiaddr::Protocol, PeerId};
use libp2p_swarm::{ ConnectionId, NetworkBehaviour, NotifyHandler, ToSwarm};

use crate::browser::{handler::{FromBehaviourEvent, SignalingHandler, ToBehaviourEvent}, transport::ConnectionRequest};

#[derive(Clone, Debug)]
pub struct SignalingConfig {
    /// Maximum number of times to retry the signaling process before giving up.
    pub(crate) max_signaling_retries: u8,
    /// Delay before initiating or retrying the signaling process.
    pub(crate) signaling_delay: Duration,
    /// Time to wait between each check for an established WebRTC connection.
    pub(crate) connection_establishment_delay_in_millis: Duration,
    /// Maximum number of checks to attempt for establishing the WebRTC connection.
    pub(crate) max_connection_establishment_checks: u32,
    /// Maximum time to wait for ICE gathering to complete before proceeding.
    pub(crate) ice_gathering_timeout: Duration,
    pub(crate) local_peer_id: PeerId,
}

// impl Default for SignalingConfig {
//     fn default() -> Self {
//         Self {
//             max_signaling_retries: 3,
//             signaling_delay: Duration::from_millis(100),
//             connection_establishment_delay_in_millis: Duration::from_millis(100),
//             max_connection_establishment_checks: 300,
//             ice_gathering_timeout: Duration::from_secs(10),
//         }
//     }
// }

impl SignalingConfig {
    pub fn new(
        max_retries: u8,
        signaling_delay: Duration,
        connection_establishment_delay_in_millis: Duration,
        max_connection_establishment_checks: u32,
        ice_gathering_timeout: Duration,
        local_peer_id: PeerId,
    ) -> Self {
        Self {
            max_signaling_retries: max_retries,
            signaling_delay,
            connection_establishment_delay_in_millis,
            max_connection_establishment_checks,
            ice_gathering_timeout,
            local_peer_id,
        }
    }
}

/// Signaling events returned to the swarm.
#[derive(Debug)]
pub enum SignalingEvent {
    /// A new WebRTC connection has been established
    NewWebRTCConnection { peer_id: PeerId },
    /// WebRTC connection establishment failed
    WebRTCConnectionError { peer_id: PeerId, error: crate::Error },
}

/// State for tracking signaling with a specific peer
#[derive(Debug)]
struct PeerSignalingState {
    /// The time at which this peer was discovered.
    discovered_at: Instant,
    /// Whether this peer initiated the signaling process.
    initiated: bool,
    // Connection ID for this peer
    connection_id: ConnectionId,
    is_relay_connection: bool,
     is_webrtc_target: bool,
}

/// A [`Behaviour`] used to coordinate signaling between peers over a relay connection.
pub struct Behaviour {
    /// Queued events to send to the swarm.
    queued_events: VecDeque<ToSwarm<SignalingEvent, FromBehaviourEvent>>,
    /// Configuration parameters for the signaling process.
    signaling_config: SignalingConfig,
    /// Tracking state of peers involved in signaling.
    peers: HashMap<PeerId, PeerSignalingState>,
    /// Pending dial requests from the transport
    pending_dials: Arc<Mutex<HashMap<PeerId, futures::channel::oneshot::Sender<crate::Connection>>>>,
    /// Established connections to inject
    established_connections: Arc<Mutex<VecDeque<ConnectionRequest>>>,
    /// Track which connections are WebRTC connections
    webrtc_connections: HashMap<ConnectionId, PeerId>,
}

impl Behaviour {
    pub fn new(
        config: SignalingConfig,
        pending_dials: Arc<Mutex<HashMap<PeerId, futures::channel::oneshot::Sender<crate::Connection>>>>,
        established_connections: Arc<Mutex<VecDeque<ConnectionRequest>>>,
    ) -> Self {
        Self {
            queued_events: VecDeque::new(),
            signaling_config: config,
            peers: HashMap::new(),
            pending_dials,
            established_connections,
            webrtc_connections: HashMap::new(),
        }
    }

     /// Handle successful WebRTC connection establishment
    async fn handle_webrtc_connection_success(
        &mut self,
        peer_id: PeerId,
        connection: crate::Connection,
    ) {
        tracing::info!("handle_webrtc_conn_success");
        // Check if there's a pending dial for this peer
        let mut pending = self.pending_dials.lock().await;
        if let Some(sender) = pending.remove(&peer_id) {
            // Outbound connection - send to waiting dial
            let _ = sender.send(connection);
        } else {
            // Inbound connection - queue for injection
            let mut established = self.established_connections.lock().await;
            established.push_back(ConnectionRequest {
                peer_id,
                connection,
            });
        }

        // Remove from signaling peers
        self.peers.remove(&peer_id);

        tracing::info!("Successful webrtc conn established.. sending peer id to swarm.");
        // Queue event for the swarm
        self.queued_events.push_back(ToSwarm::GenerateEvent(
            SignalingEvent::NewWebRTCConnection { peer_id },
        ));
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = SignalingHandler;
    type ToSwarm = SignalingEvent;

  fn handle_established_inbound_connection(
        &mut self,
        connection_id: libp2p_swarm::ConnectionId,
        peer: PeerId,
        local_addr: &libp2p_core::Multiaddr,
        remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        let is_webrtc = remote_addr.iter().any(|p| matches!(p, Protocol::WebRTC));

        if is_webrtc && !remote_addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
            // This is an established WebRTC connection
            info!("WebRTC connection established (inbound) with peer {}", peer);
            self.webrtc_connections.insert(connection_id, peer);

            // Return a no-op handler for established WebRTC connections
            Ok(SignalingHandler::new_established_webrtc(
                self.signaling_config.local_peer_id,
                peer,
            ))
        } else {
            // This is a relay connection for signaling
            info!("Creating signaling handler for inbound relay connection with peer {}", peer);
            Ok(SignalingHandler::new(
                self.signaling_config.local_peer_id,
                peer,
                false,
                self.signaling_config.clone(),
                false,
            ))
        }
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: libp2p_swarm::ConnectionId,
        peer: PeerId,
        addr: &libp2p_core::Multiaddr,
        endpoint: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        let has_circuit = addr.iter().any(|p| matches!(p, Protocol::P2pCircuit));
        let has_webrtc = addr.iter().any(|p| matches!(p, Protocol::WebRTC));

        if has_webrtc && !has_circuit {
            // This is a direct WebRTC connection (already established)
            info!("Direct WebRTC connection established (outbound) with peer {}", peer);
            self.webrtc_connections.insert(connection_id, peer);

            // Return a no-op handler for established WebRTC connections
            Ok(SignalingHandler::new_established_webrtc(
                self.signaling_config.local_peer_id,
                peer,
            ))
        } else if has_circuit && has_webrtc {
            // This is a relay connection that will be used for WebRTC signaling
            info!("Creating signaling handler for WebRTC over relay to peer {}", peer);
            // Mark this peer as a WebRTC signaling target
            self.peers.insert(
                peer,
                PeerSignalingState {
                    discovered_at: Instant::now(),
                    initiated: false,
                    connection_id,
                    is_relay_connection: true,
                    is_webrtc_target: true,  // This is the key!
                },
            );

            Ok(SignalingHandler::new(
                self.signaling_config.local_peer_id,
                peer,
                true,
                self.signaling_config.clone(),
                false,
            ))

        } else if addr.iter().any(|p| p == Protocol::P2pCircuit) {
                 // Regular relay connection that might be used for signaling (inbound WebRTC)
            info!("Relay connection established with peer {} on addr {}", peer, addr);

            self.peers.insert(
                peer,
                PeerSignalingState {
                    discovered_at: Instant::now(),
                    initiated: false,
                    connection_id,
                    is_relay_connection: true,
                    is_webrtc_target: false,
                },
            );

            Ok(SignalingHandler::new(
                self.signaling_config.local_peer_id,
                peer,
                true,
                self.signaling_config.clone(),
                false,
            ))
        } else {
            // Regular relay connection without WebRTC
            info!("Regular connection to peer {} on addr {}", peer, addr);
            Ok(SignalingHandler::new(
                self.signaling_config.local_peer_id,
                peer,
                true,
                self.signaling_config.clone(),
                true,
            ))
        }
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        match event {
            libp2p_swarm::FromSwarm::ConnectionEstablished(connection_established) => {
                tracing::info!("Connection establishment is now handled in handle_established_*_connection");
            }
            libp2p_swarm::FromSwarm::ConnectionClosed(connection_closed) => {
                let peer_id = connection_closed.peer_id;
                let connection_id = connection_closed.connection_id;

                if self.webrtc_connections.remove(&connection_id).is_some() {
                    info!("WebRTC connection {} with peer {} closed", connection_id, peer_id);
                } else if self
                    .peers
                    .get(&peer_id)
                    .map(|state| state.connection_id == connection_id && state.is_relay_connection)
                    .unwrap_or(false)
                {
                    info!("Relay connection {} with peer {} closed", connection_id, peer_id);
                    self.peers.remove(&peer_id);
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: libp2p_swarm::ConnectionId,
        event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            ToBehaviourEvent::WebRTCConnectionSuccess(connection) => {
                // Handle the connection asynchronously
                let pending_dials = self.pending_dials.clone();
                let established_connections = self.established_connections.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    // Check if there's a pending dial for this peer
                    let mut pendingf = pending_dials.lock().await;
                    if let Some(sender) = pending.remove(&peer_id) {
                        // Outbound connection - send to waiting dial
                        let _ = sender.send(connection);
                    } else {
                        // Inbound connection - queue for injection
                        let mut established = established_connections.lock().await;
                        established.push_back(ConnectionRequest {
                            peer_id,
                            connection,
                        });
                    }
                });

                // Remove from signaling peers
                self.peers.remove(&peer_id);

                // Queue event
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::NewWebRTCConnection { peer_id },
                ));
            }
            ToBehaviourEvent::WebRTCConnectionFailure(error) => {
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::WebRTCConnectionError { peer_id, error },
                ));

                // Remove peer from tracking
                self.peers.remove(&peer_id);

                // Notify any waiting dial
                let pending_dials = self.pending_dials.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let mut pending = pending_dials.lock().await;
                    pending.remove(&peer_id);
                });
            }
            ToBehaviourEvent::SignalingRetry => {
                if let Some(state) = self.peers.get_mut(&peer_id) {
                    state.initiated = false;
                    state.discovered_at = Instant::now();
                }
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        // Send any queued events
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        // Check for peers that need signaling initiation
        let now = Instant::now();
        let delay = self.signaling_config.signaling_delay;

        for (peer_id, state) in self.peers.iter_mut() {
            if !state.initiated
                && state.is_relay_connection
                && state.is_webrtc_target
                && now.duration_since(state.discovered_at) >= delay
            {
                tracing::info!("Initiating signaling with peer {}", peer_id);
                state.initiated = true;

                return Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id: *peer_id,
                    handler: NotifyHandler::One(state.connection_id),
                    event: FromBehaviourEvent::InitiateSignaling,
                });
            }
        }

        Poll::Pending
    }
}
