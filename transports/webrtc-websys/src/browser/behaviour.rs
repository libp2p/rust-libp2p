use std::{
    collections::{HashMap, VecDeque},
    str::FromStr,
    sync::Arc,
    task::Poll,
    time::Duration,
};

use futures::{channel::oneshot, lock::Mutex};
use tracing::info;
use wasm_timer::Instant;

use libp2p_core::{multiaddr::Protocol, Multiaddr, PeerId};
use libp2p_swarm::{ConnectionId, NetworkBehaviour, NotifyHandler, ToSwarm};

use crate::browser::{
    handler::{FromBehaviourEvent, SignalingHandler, ToBehaviourEvent},
    transport::ConnectionRequest,
};

use futures::task::AtomicWaker;

#[derive(Clone, Debug)]
pub struct SignalingConfig {
    pub(crate) max_signaling_retries: u8,
    pub(crate) signaling_delay: Duration,
    pub(crate) connection_establishment_delay_in_millis: Duration,
    pub(crate) max_connection_establishment_checks: u32,
    pub(crate) ice_gathering_timeout: Duration,
    pub(crate) local_peer_id: PeerId,
}

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
    WebRTCConnectionError {
        peer_id: PeerId,
        error: crate::Error,
    },
}

/// State for tracking signaling with a specific peer
#[derive(Debug)]
struct PeerSignalingState {
    discovered_at: Instant,
    initiated: bool,
    connection_id: ConnectionId,
    is_relay_connection: bool,
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
    pending_dials: Arc<Mutex<HashMap<PeerId, oneshot::Sender<crate::Connection>>>>,
    /// Established connections to inject
    established_connections: Arc<Mutex<VecDeque<ConnectionRequest>>>,
    /// Track which connections are WebRTC connections
    webrtc_connections: HashMap<ConnectionId, PeerId>,
    // Established relay connections
    established_relay_connections: HashMap<PeerId, ConnectionId>,
    transport_waker: Arc<AtomicWaker>
}

impl Behaviour {
    pub fn new(
        config: SignalingConfig,
        pending_dials: Arc<Mutex<HashMap<PeerId, oneshot::Sender<crate::Connection>>>>,
        established_connections: Arc<Mutex<VecDeque<ConnectionRequest>>>,
        transport_waker: Arc<AtomicWaker>
    ) -> Self {
        Self {
            queued_events: VecDeque::new(),
            signaling_config: config,
            peers: HashMap::new(),
            pending_dials,
            established_connections,
            webrtc_connections: HashMap::new(),
            established_relay_connections: HashMap::new(),
            transport_waker
        }
    }

    /// Handle successful WebRTC connection establishment
    async fn handle_webrtc_connection_success(
        &mut self,
        peer_id: PeerId,
        connection: crate::Connection,
    ) {
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

        if is_webrtc {
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
            info!(
                "Creating signaling handler for inbound relay connection with peer {}",
                peer
            );
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
        _endpoint: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        let has_circuit = addr.iter().any(|p| matches!(p, Protocol::P2pCircuit));
        let has_webrtc = addr.iter().any(|p| matches!(p, Protocol::WebRTC));

        if has_webrtc && !has_circuit {
            // This is a direct WebRTC connection (already established)
            info!(
                "Direct WebRTC connection established (outbound) with peer {}",
                peer
            );
            self.webrtc_connections.insert(connection_id, peer);

            // Return a no-op handler for established WebRTC connections
            Ok(SignalingHandler::new_established_webrtc(
                self.signaling_config.local_peer_id,
                peer,
            ))
        } else if has_circuit {
            info!(
                "Outbound relay connection to peer {} - preparing for WebRTC signaling",
                peer
            );

            Ok(SignalingHandler::new(
                self.signaling_config.local_peer_id,
                peer,
                true, // is_dialer
                self.signaling_config.clone(),
                false,
            ))
        } else {
            // Direct non-WebRTC connection
            info!(
                "Other outbound connection to peer {} on addr {}",
                peer, addr
            );
            Ok(SignalingHandler::new(
                self.signaling_config.local_peer_id,
                peer,
                true,
                self.signaling_config.clone(),
                true, // noop for other connections
            ))
        }
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        tracing::info!("on_swarm_event {:?}", event);
        match event {
            libp2p_swarm::FromSwarm::ConnectionEstablished(connection_established) => {
                tracing::info!(
                    "Swarm event.. conn established: {:?}",
                    connection_established
                );

                let peer_id = connection_established.peer_id;
                let connection_id = connection_established.connection_id;
                let endpoint = connection_established.endpoint;
                let addr = endpoint.get_remote_address();

                // Only track relay connections for signaling
                if endpoint.is_relayed() && !self.webrtc_connections.contains_key(&connection_id) {
                    self.established_relay_connections
                        .insert(peer_id, connection_id);

                    self.peers.insert(
                        peer_id,
                        PeerSignalingState {
                            discovered_at: Instant::now(),
                            initiated: false,
                            connection_id,
                            is_relay_connection: true,
                        },
                    );
                }
            }
            libp2p_swarm::FromSwarm::ConnectionClosed(connection_closed) => {
                let peer_id = connection_closed.peer_id;
                let connection_id = connection_closed.connection_id;


                // Check if this was a WebRTC connection
                if self.webrtc_connections.remove(&connection_id).is_some() {
                    info!(
                        "WebRTC connection {} with peer {} closed",
                        connection_id, peer_id
                    );
                } else {
                    // Check if this was a relay connection used for signaling
                    if self
                        .peers
                        .get(&peer_id)
                        .map(|state| {
                            state.connection_id == connection_id && state.is_relay_connection
                        })
                        .unwrap_or(false)
                    {
                        info!(
                            "Relay connection {} with peer {} closed (signaling state removed)",
                            connection_id, peer_id
                        );
                        self.peers.remove(&peer_id);
                    }
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
let transport_waker = self.transport_waker.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    // Check if there's a pending dial for this peer
                    let mut pending = pending_dials.lock().await;
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

                        transport_waker.wake();
                    }
                });

                // Remove from signaling peers
                self.peers.remove(&peer_id);

                // Queue event
                tracing::info!(
                    "Sending signal event - NewWebRTCConnection to swarm for peer {peer_id}"
                );
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::NewWebRTCConnection { peer_id },
                ));

                tracing::info!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
                if let Some(relay_conn_id) = self.established_relay_connections.remove(&peer_id) {
                    tracing::info!(
                        "Closing relay connection {} to {} (WebRTC established)",
                        relay_conn_id,
                        peer_id
                    );
                    self.queued_events.push_back(ToSwarm::CloseConnection {
                        peer_id,
                        connection: libp2p_swarm::CloseConnection::One(relay_conn_id),
                    });
                }
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
        tracing::info!("Polling in behaviour.");
        // Send any queued events
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        // Check for peers that need signaling initiation
        let now = Instant::now();
        let delay = self.signaling_config.signaling_delay;

        tracing::info!(
            "Poll: checking {} peers for signaling initiation",
            self.peers.len()
        );
        for (peer_id, state) in self.peers.iter_mut() {
            tracing::info!(
                "  Peer {}: initiated={}, is_relay={}, time_elapsed={:?}",
                peer_id,
                state.initiated,
                state.is_relay_connection,
                now.duration_since(state.discovered_at)
            );

            if !state.initiated
                && state.is_relay_connection
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
