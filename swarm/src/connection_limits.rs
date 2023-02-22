// Copyright 2023 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use crate::{
    dummy, ConnectionClosed, ConnectionDenied, ConnectionId, ConnectionLimit, ConnectionLimits,
    FromSwarm, NetworkBehaviour, NetworkBehaviourAction, PollParameters, THandler, THandlerInEvent,
    THandlerOutEvent,
};
use libp2p_core::{Endpoint, Multiaddr, PeerId};
use std::collections::{HashMap, HashSet};
use std::task::{Context, Poll};
use void::Void;

pub struct Behaviour {
    limits: ConnectionLimits,

    pending_inbound_connections: HashSet<ConnectionId>,
    pending_outbound_connections: HashSet<ConnectionId>,
    established_inbound_connections: HashSet<ConnectionId>,
    established_outbound_connections: HashSet<ConnectionId>,
    established_per_peer: HashMap<PeerId, HashSet<ConnectionId>>,
}

impl Behaviour {
    pub fn new(limits: ConnectionLimits) -> Self {
        Self {
            limits,
            pending_inbound_connections: Default::default(),
            pending_outbound_connections: Default::default(),
            established_inbound_connections: Default::default(),
            established_outbound_connections: Default::default(),
            established_per_peer: Default::default(),
        }
    }

    fn check_limit(&mut self, limit: Option<u32>, current: usize) -> Result<(), ConnectionDenied> {
        let limit = limit.unwrap_or(u32::MAX);
        let current = current as u32;

        if current > limit {
            return Err(ConnectionDenied::new(ConnectionLimit { limit, current }));
        }

        Ok(())
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type OutEvent = Void;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.check_limit(
            self.limits.max_pending_incoming,
            self.pending_inbound_connections.len(),
        )?;

        self.pending_inbound_connections.insert(connection_id);

        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        peer: PeerId,
        connection_id: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_inbound_connections.remove(&connection_id);

        self.check_limit(
            self.limits.max_established_incoming,
            self.established_inbound_connections.len(),
        )?;
        self.check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
        )?;
        self.check_limit(
            self.limits.max_established_total,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
        )?;

        self.established_inbound_connections.insert(connection_id);
        self.established_per_peer
            .entry(peer)
            .or_default()
            .insert(connection_id);

        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
        connection_id: ConnectionId,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.check_limit(
            self.limits.max_pending_outgoing,
            self.pending_outbound_connections.len(),
        )?;

        self.pending_outbound_connections.insert(connection_id);

        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        connection_id: ConnectionId,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_outbound_connections.remove(&connection_id);

        self.check_limit(
            self.limits.max_established_outgoing,
            self.established_outbound_connections.len(),
        )?;
        self.check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
        )?;
        self.check_limit(
            self.limits.max_established_total,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
        )?;

        self.established_outbound_connections.insert(connection_id);
        self.established_per_peer
            .entry(peer)
            .or_default()
            .insert(connection_id);

        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                self.established_inbound_connections.remove(&connection_id);
                self.established_outbound_connections.remove(&connection_id);
                self.established_per_peer
                    .entry(peer_id)
                    .or_default()
                    .remove(&connection_id);
            }
            FromSwarm::ConnectionEstablished(_) => {}
            FromSwarm::AddressChange(_) => {}
            FromSwarm::DialFailure(_) => {}
            FromSwarm::ListenFailure(_) => {}
            FromSwarm::NewListener(_) => {}
            FromSwarm::NewListenAddr(_) => {}
            FromSwarm::ExpiredListenAddr(_) => {}
            FromSwarm::ListenerError(_) => {}
            FromSwarm::ListenerClosed(_) => {}
            FromSwarm::NewExternalAddr(_) => {}
            FromSwarm::ExpiredExternalAddr(_) => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}
