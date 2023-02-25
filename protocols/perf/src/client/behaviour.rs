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

use std::task::{Context, Poll};

use libp2p_core::PeerId;
use libp2p_swarm::{
    dummy, ConnectionId, FromSwarm, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
    THandlerInEvent, THandlerOutEvent,
};

use crate::client::handler::Handler;

pub enum Event {}

pub struct Behaviour {}

impl Behaviour {
    pub fn new() -> Self {
        todo!();
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type OutEvent = Event;

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        todo!();
    }

    fn on_connection_handler_event(
        &mut self,
        event_source: PeerId,
        connection_id: ConnectionId,
        handler_event: THandlerOutEvent<Self>,
    ) {
        todo!();
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, THandlerInEvent<Self>>> {
        todo!();
    }
}
