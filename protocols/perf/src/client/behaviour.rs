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

//! [`NetworkBehaviour`] of the libp2p perf protocol.

use std::{
    collections::{HashSet, VecDeque},
    task::{Context, Poll},
};

use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{
    derive_prelude::ConnectionEstablished, ConnectionClosed, ConnectionId, FromSwarm,
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, THandlerInEvent,
    THandlerOutEvent,
};

use crate::client::handler::Handler;

use super::{RunParams, RunStats};

#[derive(Debug)]
pub enum Event {
    Finished { stats: RunStats },
}

#[derive(Default)]
pub struct Behaviour {
    /// Queue of actions to return when polled.
    queued_events: VecDeque<NetworkBehaviourAction<Event, THandlerInEvent<Self>>>,
    /// Set of connected peers.
    connected: HashSet<PeerId>,
}

impl Behaviour {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn perf(&mut self, server: PeerId, params: RunParams) -> Result<(), PerfError> {
        if !self.connected.contains(&server) {
            return Err(PerfError::NotConnected);
        }

        self.queued_events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: server,
                handler: NotifyHandler::Any,
                event: crate::client::handler::Command::Start { params },
            });

        return Ok(());
    }
}

#[derive(thiserror::Error, Debug)]
pub enum PerfError {
    #[error("Not connected to peer")]
    NotConnected,
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type OutEvent = Event;

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p_core::Endpoint,
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

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id: _,
                endpoint: _,
                failed_addresses: _,
                other_established: _,
            }) => {
                self.connected.insert(peer_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id: _,
                endpoint: _,
                handler: _,
                remaining_established,
            }) => {
                if remaining_established == 0 {
                    assert!(self.connected.remove(&peer_id));
                }
            }
            FromSwarm::AddressChange(_) => todo!(),
            FromSwarm::DialFailure(_) => todo!(),
            FromSwarm::ListenFailure(_) => todo!(),
            FromSwarm::NewListener(_) => todo!(),
            FromSwarm::NewListenAddr(_) => todo!(),
            FromSwarm::ExpiredListenAddr(_) => todo!(),
            FromSwarm::ListenerError(_) => todo!(),
            FromSwarm::ListenerClosed(_) => todo!(),
            FromSwarm::NewExternalAddr(_) => todo!(),
            FromSwarm::ExpiredExternalAddr(_) => todo!(),
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _event_source: PeerId,
        _connection_id: ConnectionId,
        handler_event: THandlerOutEvent<Self>,
    ) {
        match handler_event {
            super::handler::Event::Finished { stats } => {
                self.queued_events
                    .push_back(NetworkBehaviourAction::GenerateEvent(Event::Finished {
                        stats,
                    }));
            }
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
