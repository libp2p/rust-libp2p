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
    collections::{HashMap, VecDeque},
    task::{Context, Poll},
};

use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{
    derive_prelude::ConnectionEstablished, dial_opts::DialOpts, ConnectionId, FromSwarm,
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, THandlerInEvent,
    THandlerOutEvent,
};

use crate::{client::handler::Handler, RunParams, RunStats};

#[derive(Debug)]
pub enum Event {
    Finished { stats: RunStats },
}

#[derive(Default)]
pub struct Behaviour {
    pending_run: HashMap<ConnectionId, RunParams>,
    /// Queue of actions to return when polled.
    queued_events: VecDeque<NetworkBehaviourAction<Event, THandlerInEvent<Self>>>,
}

impl Behaviour {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn perf(&mut self, server: Multiaddr, params: RunParams) {
        let opts: DialOpts = server.into();
        let connection_id = opts.connection_id();

        self.pending_run.insert(connection_id, params);

        // TODO: What if we are already connected?
        self.queued_events
            .push_back(NetworkBehaviourAction::Dial { opts });
    }
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

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint: _,
                failed_addresses: _,
                other_established: _,
            }) => self
                .queued_events
                .push_back(NetworkBehaviourAction::NotifyHandler {
                    peer_id,
                    handler: NotifyHandler::One(connection_id),
                    event: crate::client::handler::Command::Start {
                        started_at: std::time::Instant::now(),
                        params: self.pending_run.remove(&connection_id).unwrap(),
                    },
                }),
            FromSwarm::ConnectionClosed(_) => todo!(),
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
