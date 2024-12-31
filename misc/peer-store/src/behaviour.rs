use std::{
    collections::{HashSet, VecDeque},
    task::Poll,
    time::Duration,
};

use futures_timer::Delay;
use futures_util::FutureExt;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{dummy, FromSwarm, NetworkBehaviour};

use crate::store::{AddressSource, Store};

/// Events generated by [`Behaviour`] and emitted back to the [`libp2p_swarm::Swarm`].
pub enum Event {
    /// The peer's record has been updated.  
    /// Manually updating a record will always emit this event
    /// even if it provides no new information.
    RecordUpdated { peer: PeerId },
}

pub struct Config {
    /// The interval for garbage collecting records.
    check_record_ttl_interval: Duration,
}

pub struct Behaviour<S> {
    store: S,
    /// Peers that are currently connected.
    connected_peers: HashSet<PeerId>,
    /// Pending Events to be emitted back to the  [`libp2p_swarm::Swarm`].
    pending_events: VecDeque<Event>,
    record_ttl_timer: Option<Delay>,
    config: Config,
}

impl<'a, S> Behaviour<S>
where
    S: Store<'a> + 'static,
{
    pub fn new(store: S, config: Config) -> Self {
        Self {
            store,
            connected_peers: HashSet::new(),
            pending_events: VecDeque::new(),
            record_ttl_timer: Some(Delay::new(config.check_record_ttl_interval)),
            config,
        }
    }

    /// List peers that are currently connected to this peer.
    pub fn list_connected(&self) -> impl Iterator<Item = &PeerId> {
        self.connected_peers.iter()
    }

    /// Try to get all observed address of the given peer.  
    /// Returns `None` when the peer is not in the store.
    pub fn address_of_peer<'b>(
        &'a self,
        peer: &'b PeerId,
    ) -> Option<impl Iterator<Item = &'a Multiaddr> + use<'a, 'b, S>> {
        self.store.addresses_of_peer(peer)
    }

    /// Manually update a record.  
    /// This will always emit an `Event::RecordUpdated`.
    pub fn update_address(&mut self, peer: &PeerId, address: &Multiaddr) {
        self.store
            .update_address(peer, address, AddressSource::Manual, false);
        self.pending_events
            .push_back(Event::RecordUpdated { peer: *peer });
    }

    /// Should be called when other protocol emits a [`PeerRecord`](libp2p_core::PeerRecord).  
    /// This will always emit an `Event::RecordUpdated`.
    pub fn on_signed_peer_record(
        &mut self,
        peer: &PeerId,
        record: libp2p_core::PeerRecord,
        source: AddressSource,
    ) {
        self.store
            .update_certified_address(peer, record, source, false);
        self.pending_events
            .push_back(Event::RecordUpdated { peer: *peer });
    }

    /// Get a immutable reference to the internal store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Get a mutable reference to the internal store.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    fn on_peer_connect(&mut self, peer: &PeerId) {
        self.connected_peers.insert(*peer);
    }
    fn on_peer_disconnect(&mut self, peer: &PeerId) {
        self.connected_peers.remove(peer);
    }
    fn on_address_update(
        &mut self,
        peer: &PeerId,
        address: &Multiaddr,
        source: AddressSource,
        should_expire: bool,
    ) {
        if self
            .store
            .update_address(peer, address, source, should_expire)
        {
            self.pending_events
                .push_back(Event::RecordUpdated { peer: *peer });
        }
    }
    fn handle_store_event(&mut self, event: super::store::Event) {
        use super::store::Event::*;
        match event {
            RecordUpdated(peer) => self.pending_events.push_back(Event::RecordUpdated { peer }),
        }
    }
}

impl<'a, S> NetworkBehaviour for Behaviour<S>
where
    S: Store<'a> + 'static,
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
        self.on_address_update(&peer, remote_addr, AddressSource::DirectConnection, false);
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
            return Ok(Vec::new());
        }
        let peer = maybe_peer.expect("already handled");
        if let Some(unsigned) = self.store.addresses_of_peer(&peer) {
            if let Some(signed) = self.store.certified_addresses_of_peer(&peer) {
                return Ok(signed.chain(unsigned).cloned().collect());
            }
            return Ok(unsigned.cloned().collect());
        }
        Ok(Vec::new())
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        peer: libp2p_core::PeerId,
        addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        self.on_address_update(&peer, addr, AddressSource::DirectConnection, false);
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        if let Some(ev) = self.store.on_swarm_event(&event) {
            self.handle_store_event(ev);
        };
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
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        if let Some(ev) = self.pending_events.pop_front() {
            return Poll::Ready(libp2p_swarm::ToSwarm::GenerateEvent(ev));
        }
        self.poll_record_ttl(cx);
        Poll::Pending
    }
}

impl<'a, S> Behaviour<S>
where
    S: Store<'a>,
{
    /// Garbage collect records.
    fn poll_record_ttl(&mut self, cx: &mut std::task::Context<'_>) {
        if let Some(mut timer) = self.record_ttl_timer.take() {
            if let Poll::Ready(()) = timer.poll_unpin(cx) {
                self.store.check_ttl();
                self.record_ttl_timer = Some(Delay::new(self.config.check_record_ttl_interval));
            }
            self.record_ttl_timer = Some(timer)
        }
    }
}
