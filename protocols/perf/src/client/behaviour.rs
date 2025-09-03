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

//! [`NetworkBehaviour`] of the libp2p perf client protocol.

use std::{
    collections::{HashSet, VecDeque},
    task::{Context, Poll},
};

use libp2p_core::{transport::PortUse, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionId, FromSwarm,
    NetworkBehaviour, NotifyHandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};

use super::{RunError, RunId};
use crate::{client::handler::Handler, RunParams, RunUpdate};

#[derive(Debug)]
pub struct Event {
    pub id: RunId,
    pub result: Result<RunUpdate, RunError>,
}

#[derive(Default)]
pub struct Behaviour {
    /// Queue of actions to return when polled.
    queued_events: VecDeque<ToSwarm<Event, THandlerInEvent<Self>>>,
    /// Set of connected peers.
    connected: HashSet<PeerId>,
}

impl Behaviour {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn perf(&mut self, server: PeerId, params: RunParams) -> Result<RunId, NotConnected> {
        if !self.connected.contains(&server) {
            return Err(NotConnected {});
        }

        let id = RunId::next();

        self.queued_events.push_back(ToSwarm::NotifyHandler {
            peer_id: server,
            handler: NotifyHandler::Any,
            event: crate::client::handler::Command { id, params },
        });

        Ok(id)
    }
}

#[derive(thiserror::Error, Debug)]
pub struct NotConnected();

impl std::fmt::Display for NotConnected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not connected to peer")
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(Handler::default())
    }

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(Handler::default())
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished { peer_id, .. }) => {
                self.connected.insert(peer_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id: _,
                endpoint: _,
                remaining_established,
                ..
            }) => {
                if remaining_established == 0 {
                    assert!(self.connected.remove(&peer_id));
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _event_source: PeerId,
        _connection_id: ConnectionId,
        super::handler::Event { id, result }: THandlerOutEvent<Self>,
    ) {
        self.queued_events
            .push_back(ToSwarm::GenerateEvent(Event { id, result }));
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self))]
    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
