use std::{
    collections::{HashSet, VecDeque},
    task::Poll,
};

use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{dummy, FromSwarm, NetworkBehaviour};

use crate::store::Store;

/// Events of this behaviour that will be emmitted to the swarm.
pub enum Event {
    RecordUpdated { peer: PeerId },
}

pub struct Behaviour<S> {
    store: S,
    /// Peers that are currently connected.
    connected_peers: HashSet<PeerId>,
    /// Events that will be emitted.
    pending_events: VecDeque<Event>,
}

impl<S> Behaviour<S>
where
    S: Store + 'static,
{
    pub fn new(store: S) -> Self {
        Self {
            store,
            connected_peers: HashSet::new(),
            pending_events: VecDeque::new(),
        }
    }
    /// List peers that are currently connected to this peer.
    pub fn list_connected(&self) -> impl Iterator<Item = &PeerId> {
        self.connected_peers.iter()
    }
    /// Try to get all observed address of the given peer.  
    /// Returns `None` when the peer is not in the store.
    pub fn address_of_peer<'a, 'b>(
        &'a self,
        peer: &'b PeerId,
    ) -> Option<impl Iterator<Item = super::AddressRecord<'a>> + use<'a, 'b, S>> {
        self.store.addresses_of_peer(peer)
    }
    /// Manually update a record.  
    /// This will always cause an `Event::RecordUpdated` to be emitted.
    pub fn update_record(&mut self, peer: &PeerId, address: &Multiaddr) {
        self.store.on_address_update(peer, address);
        self.pending_events
            .push_back(Event::RecordUpdated { peer: *peer });
    }
    fn on_peer_connect(&mut self, peer: &PeerId) {
        self.connected_peers.insert(*peer);
    }
    fn on_peer_disconnect(&mut self, peer: &PeerId) {
        self.connected_peers.remove(peer);
    }
    fn on_address_update(&mut self, peer: &PeerId, address: &Multiaddr) {
        if self.store.on_address_update(peer, address) {
            self.pending_events
                .push_back(Event::RecordUpdated { peer: *peer });
        }
    }
}

impl<S> NetworkBehaviour for Behaviour<S>
where
    S: Store + 'static,
{
    type ConnectionHandler = dummy::ConnectionHandler;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        peer: libp2p_core::PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        self.on_address_update(&peer, remote_addr);
        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: libp2p_core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p_swarm::ConnectionDenied> {
        if maybe_peer.is_none() {
            return Ok(Vec::with_capacity(0));
        }
        if let Some(i) = self
            .store
            .addresses_of_peer(&maybe_peer.expect("already handled"))
        {
            return Ok(i.map(|r| r.address).cloned().collect());
        }
        Ok(Vec::with_capacity(0))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        peer: libp2p_core::PeerId,
        addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        self.on_address_update(&peer, addr);
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        match event {
            FromSwarm::ConnectionClosed(info) => {
                if info.remaining_established < 1 {
                    self.on_peer_disconnect(&info.peer_id);
                }
            }
            FromSwarm::ConnectionEstablished(info) => {
                if info.other_established == 0 {
                    self.on_peer_connect(&info.peer_id);
                }
            }
            FromSwarm::NewExternalAddrOfPeer(info) => {
                self.on_address_update(&info.peer_id, info.addr);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p_core::PeerId,
        _connection_id: libp2p_swarm::ConnectionId,
        _event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        unreachable!("No event will be produced by a dummy handler.")
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        if let Some(ev) = self.pending_events.pop_front() {
            return Poll::Ready(libp2p_swarm::ToSwarm::GenerateEvent(ev));
        }
        Poll::Pending
    }
}
