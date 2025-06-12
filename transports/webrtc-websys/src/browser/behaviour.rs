use std::{
    collections::{HashMap, VecDeque},
    task::Poll,
    time::Duration
};

use wasm_timer::Instant;

use libp2p_core::PeerId;
use libp2p_swarm::{ConnectionId, NetworkBehaviour, NotifyHandler, ToSwarm};

use crate::browser::handler::{FromBehaviourEvent, SignalingHandler, ToBehaviourEvent};

#[derive(Clone, Debug)]
pub struct SignalingConfig {
    /// Maximum number of times to retry the signaling process before giving up.
    pub(crate) max_signaling_retries: u8,
    /// Delay before initiating or retrying the signaling process.
    pub(crate) signaling_delay: Duration,
    /// Time to wait between each check for an established WebRTC connection.
    pub(crate) connection_establishment_delay_in_millis: Duration,
    /// Maximum number of checks to attempt for establishing the WebRTC connection.
    pub(crate) max_connection_establishment_checks: u32
}

impl Default for SignalingConfig {
    fn default() -> Self {
        Self {
            max_signaling_retries: 3,
            signaling_delay: Duration::from_millis(100),
            connection_establishment_delay_in_millis: Duration::from_millis(100),
            max_connection_establishment_checks: 300
        }
    }
}

impl SignalingConfig {
    pub fn new(max_retries: u8, signaling_delay: Duration, connection_establishment_delay_in_millis: Duration, max_connection_establishment_checks: u32) -> Self {
        Self {
            max_signaling_retries: max_retries,
            signaling_delay,
            connection_establishment_delay_in_millis,
            max_connection_establishment_checks
        }
    }
}

/// Signaling events returned to the swarm.
#[derive(Debug)]
pub enum SignalingEvent {
    NewWebRTCConnection(crate::Connection),
    WebRTCConnectionError(crate::Error),
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
}

/// A [`Behaviour`] used to cooordinate signaling between peers
/// over a relay connection.
pub struct Behaviour {
    /// Queued events to send to the swarm.
    queued_events: VecDeque<ToSwarm<SignalingEvent, FromBehaviourEvent>>,
    /// Configuration parameters for the signaling process.
    signaling_config: SignalingConfig,
    /// Tracking state of peers involved in signaling (to be signaled or already signaled).
    peers: HashMap<PeerId, PeerSignalingState>,
}

impl Behaviour {
    pub fn new(config: SignalingConfig) -> Self {
        Self {
            queued_events: VecDeque::new(),
            signaling_config: config,
            peers: HashMap::new(),
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = SignalingHandler;
    type ToSwarm = SignalingEvent;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        peer: PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(SignalingHandler::new(
            peer,
            false,
            self.signaling_config.clone(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        peer: PeerId,
        _addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(SignalingHandler::new(
            peer,
            true,
            self.signaling_config.clone(),
        ))
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        match event {
            libp2p_swarm::FromSwarm::ConnectionEstablished(connection_established) => {
                let dst_peer = connection_established.peer_id;
                let connection_id = connection_established.connection_id;
                let endpoint = connection_established.endpoint;

                // Check to see if the connected was made over a relay. If so, we know this is
                // the connection we need to initiate the signaling protocol.
                if endpoint.is_relayed() {
                    self.peers.insert(
                        dst_peer,
                        PeerSignalingState {
                            discovered_at: Instant::now(),
                            initiated: false,
                            connection_id,
                        },
                    );
                }
            }
            libp2p_swarm::FromSwarm::ConnectionClosed(connection_closed) => {
                if self
                    .peers
                    .get(&connection_closed.peer_id)
                    .map(|state| state.connection_id == connection_closed.connection_id)
                    .unwrap_or(false)
                {
                    self.peers.remove(&connection_closed.peer_id);
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
                tracing::info!("Successfully webrtc connection to peer {}", peer_id);
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::NewWebRTCConnection(connection),
                ));
                self.peers.remove(&peer_id);
            }
            ToBehaviourEvent::WebRTCConnectionFailure(error) => {
                tracing::error!("WebRTC connection failed: {:?}", error);
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::WebRTCConnectionError(error),
                ));
                self.peers.remove(&peer_id);
            }
            ToBehaviourEvent::SignalingRetry => {
                tracing::info!("Scheduling signaling retry for peer {}", peer_id);
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
        // Check if there are any queued events to send to the swarm
        if !self.queued_events.is_empty() {
            if let Some(event) = self.queued_events.pop_front() {
                return Poll::Ready(event);
            }
        }

        let now = Instant::now();
        let delay = self.signaling_config.signaling_delay;

        for (peer_id, state) in self.peers.iter_mut() {
            if !state.initiated && now.duration_since(state.discovered_at) >= delay {
                tracing::info!("Initiated signaling with peer {}", peer_id);
                state.initiated = true;

                return Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id: peer_id.clone(),
                    handler: NotifyHandler::One(state.connection_id),
                    event: FromBehaviourEvent::InitiateSignaling,
                });
            }
        }

        Poll::Pending
    }
}
