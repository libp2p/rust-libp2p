use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    task::Poll,
    time::Duration,
};

use futures::{channel::oneshot, task::AtomicWaker};
use libp2p_core::{multiaddr::Protocol, PeerId};
use libp2p_swarm::{ConnectionId, NetworkBehaviour, NotifyHandler, ToSwarm};
use tracing::info;
use web_time::Instant;

use crate::browser::{
    handler::{FromBehaviourEvent, SignalingHandler, ToBehaviourEvent},
    transport::ConnectionRequest,
};

#[derive(Clone, Debug)]
pub struct SignalingConfig {
    pub(crate) max_signaling_retries: u8,
    pub(crate) signaling_delay: Duration,
    pub(crate) connection_establishment_delay_in_millis: Duration,
    pub(crate) max_connection_establishment_checks: u32,
    pub(crate) stun_servers: Option<Vec<String>>,
}

impl SignalingConfig {
    pub fn new(
        max_retries: u8,
        signaling_delay: Duration,
        connection_establishment_delay_in_millis: Duration,
        max_connection_establishment_checks: u32,
        stun_servers: Option<Vec<String>>,
    ) -> Self {
        Self {
            max_signaling_retries: max_retries,
            signaling_delay,
            connection_establishment_delay_in_millis,
            max_connection_establishment_checks,
            stun_servers,
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
    /// Established relay connections
    established_relay_connections: HashMap<PeerId, ConnectionId>,
    transport_waker: Arc<AtomicWaker>,
}

impl Behaviour {
    pub fn new(
        config: SignalingConfig,
        pending_dials: Arc<Mutex<HashMap<PeerId, oneshot::Sender<crate::Connection>>>>,
        established_connections: Arc<Mutex<VecDeque<ConnectionRequest>>>,
        transport_waker: Arc<AtomicWaker>,
    ) -> Self {
        Self {
            queued_events: VecDeque::new(),
            signaling_config: config,
            peers: HashMap::new(),
            pending_dials,
            established_connections,
            webrtc_connections: HashMap::new(),
            established_relay_connections: HashMap::new(),
            transport_waker,
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = SignalingHandler;
    type ToSwarm = SignalingEvent;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: libp2p_swarm::ConnectionId,
        peer: PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        let is_webrtc_conn = remote_addr.iter().any(|p| matches!(p, Protocol::WebRTC));

        if is_webrtc_conn {
            tracing::trace!("WebRTC connection established (inbound) with peer {}", peer);
            self.webrtc_connections.insert(connection_id, peer);

            // Return a no-op handler for established WebRTC connections
            Ok(SignalingHandler::new_established_webrtc(peer))
        } else {
            Ok(SignalingHandler::new(
                peer,
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
            tracing::trace!(
                "Direct WebRTC connection established (outbound) with peer {}",
                peer
            );
            self.webrtc_connections.insert(connection_id, peer);

            // Return a no-op handler for established WebRTC connections
            Ok(SignalingHandler::new_established_webrtc(peer))
        } else if has_circuit {
            info!(
                "Outbound relay connection to peer {} - preparing for WebRTC signaling",
                peer
            );

            Ok(SignalingHandler::new(
                peer,
                self.signaling_config.clone(),
                false,
            ))
        } else {
            tracing::info!(
                "Other outbound connection to peer {} on addr {}",
                peer,
                addr
            );
            Ok(SignalingHandler::new(
                peer,
                self.signaling_config.clone(),
                true,
            ))
        }
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        match event {
            libp2p_swarm::FromSwarm::ConnectionEstablished(connection_established) => {
                let peer_id = connection_established.peer_id;
                let connection_id = connection_established.connection_id;
                let endpoint = connection_established.endpoint;

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

                if self.webrtc_connections.remove(&connection_id).is_some() {
                    info!(
                        "WebRTC connection {} with peer {} closed",
                        connection_id, peer_id
                    );
                } else if self
                    .peers
                    .get(&peer_id)
                    .map(|state| state.connection_id == connection_id && state.is_relay_connection)
                    .unwrap_or(false)
                {
                    tracing::debug!(
                        "Relay connection {} with peer {} closed (signaling state removed)",
                        connection_id,
                        peer_id
                    );

                    self.peers.remove(&peer_id);
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _connection_id: libp2p_swarm::ConnectionId,
        event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            ToBehaviourEvent::WebRTCConnectionSuccess(connection) => {
                let mut pending = self
                    .pending_dials
                    .lock()
                    .expect("pending_dials mutex poisoned");
                if let Some(sender) = pending.remove(&peer_id) {
                    let _ = sender.send(connection);
                } else {
                    tracing::info!(
                        "No pending dial found, treating as inbound for peer {}",
                        peer_id
                    );

                    drop(pending);
                    self.established_connections
                        .lock()
                        .expect("established_connections mutex poisoned")
                        .push_back(ConnectionRequest {
                            peer_id,
                            connection,
                        });
                    self.transport_waker.wake();
                }

                self.peers.remove(&peer_id);

                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::NewWebRTCConnection { peer_id },
                ));
            }
            ToBehaviourEvent::WebRTCConnectionFailure(error) => {
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::WebRTCConnectionError { peer_id, error },
                ));

                self.peers.remove(&peer_id);

                self.pending_dials
                    .lock()
                    .expect("pending_dials mutex poisoned")
                    .remove(&peer_id);
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
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        let now = Instant::now();
        let delay = self.signaling_config.signaling_delay;

        for (peer_id, state) in self.peers.iter_mut() {
            tracing::trace!(
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
                // Whoever requested the dial owns the initiator role. The spec's
                // "convention to prevent both sides initiating" only protects the
                // case where both peers dialed each other simultaneously; in the
                // common asymmetric case, deferring on the dialer side would
                // deadlock because the remote has no reason to initiate.
                let is_dialer = self
                    .pending_dials
                    .lock()
                    .expect("pending_dials mutex poisoned")
                    .contains_key(peer_id);

                state.initiated = true;

                return Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id: *peer_id,
                    handler: NotifyHandler::One(state.connection_id),
                    event: FromBehaviourEvent::InitiateSignaling { is_dialer },
                });
            }
        }

        Poll::Pending
    }
}
