use std::{
    collections::VecDeque,
    task::{Context, Poll},
};

use libp2p_core::transport::PortUse;
use libp2p_core::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionClosed, ConnectionEstablished, FromSwarm},
    ConnectionDenied, ConnectionId, NetworkBehaviour, THandler, THandlerInEvent, THandlerOutEvent,
    ToSwarm,
};
use tracing::{debug, trace};

use crate::{store::ConnectionStore, Event};

/// A [`NetworkBehaviour`] that tracks connected peers.
///
/// This behaviour provides simple connection tracking without any advanced
/// features like banning or scoring (for now). It's designed to be lightweight and
/// composable with other behaviours.
///
/// # Example
///
/// ```rust
/// use libp2p_connection_tracker::Behaviour;
/// use libp2p_swarm_derive::NetworkBehaviour;
///
/// #[derive(NetworkBehaviour)]
/// #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// struct MyBehaviour {
///     connection_tracker: Behaviour,
///     // ... other behaviours
/// }
///
/// let connection_tracker = Behaviour::new();
/// ```
pub struct Behaviour {
    /// Storage for connection state.
    store: ConnectionStore,

    /// Queue of events to emit.
    pending_events: VecDeque<Event>,
}

impl Behaviour {
    /// Create a new connection tracker behaviour.
    pub fn new() -> Self {
        Self {
            store: ConnectionStore::new(),
            pending_events: VecDeque::new(),
        }
    }

    /// Check if a peer is currently connected.
    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.store.is_connected(peer_id)
    }

    /// Get all currently connected peer IDs.
    pub fn connected_peers(&self) -> impl Iterator<Item = &PeerId> {
        self.store.connected_peers()
    }

    /// Get the number of currently connected peers.
    pub fn connected_count(&self) -> usize {
        self.store.connected_count()
    }

    /// Get the number of connections to a specific peer.
    pub fn connection_count(&self, peer_id: &PeerId) -> usize {
        self.store.connection_count(peer_id)
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = libp2p_swarm::dummy::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_pending_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        // We don't interfere with connection establishment
        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(libp2p_swarm::dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _maybe_peer: Option<PeerId>,
        _addresses: &[libp2p_core::Multiaddr],
        _effective_role: libp2p_core::Endpoint,
    ) -> Result<Vec<libp2p_core::Multiaddr>, ConnectionDenied> {
        // Don't modify addresses
        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(libp2p_swarm::dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            }) => {
                trace!(%peer_id, ?connection_id, "Connection established");

                let is_first_connection = self.store.connection_established(peer_id, connection_id);

                if is_first_connection {
                    debug!(?peer_id, "Peer connected");
                    self.pending_events.push_back(Event::PeerConnected {
                        peer_id,
                        connection_id,
                        endpoint: endpoint.clone(),
                    });
                }
            }

            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                remaining_established,
                ..
            }) => {
                trace!(%peer_id, ?connection_id, remaining_established, "Connection closed");

                let is_last_connection =
                    self.store
                        .connection_closed(peer_id, connection_id, remaining_established);

                if is_last_connection {
                    debug!(%peer_id, "Peer disconnected");
                    self.pending_events.push_back(Event::PeerDisconnected {
                        peer_id,
                        connection_id,
                    });
                }
            }

            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {}
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }
}

impl Default for Behaviour {
    fn default() -> Self {
        Self::new()
    }
}
