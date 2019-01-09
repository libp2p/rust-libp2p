// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Handles the `/ipfs/ping/1.0.0` protocol. This allows pinging a remote node and waiting for an
//! answer.
//!
//! # Usage
//!
//! The `Ping` struct implements the `NetworkBehaviour` trait. When used, it will automatically
//! send a periodic ping to nodes we are connected to. If a remote doesn't answer in time, it gets
//! automatically disconnected.
//!
//! The `Ping` struct is also what handles answering to the pings sent by remotes.
//!
//! When a ping succeeds, a `PingSuccess` event is generated, indicating the time the ping took.

pub mod dial_handler;
pub mod listen_handler;
pub mod protocol;

use futures::prelude::*;
use libp2p_core::either::EitherOutput;
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, protocols_handler::ProtocolsHandlerSelect, PeerId};
use std::{marker::PhantomData, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite};

/// Network behaviour that handles receiving pings sent by other nodes and periodically pings the
/// nodes we are connected to.
///
/// See the crate root documentation for more information.
pub struct Ping<TSubstream> {
    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
    /// Queue of events to report to the user.
    events: Vec<PingEvent>,
}

/// Event generated by the `Ping` behaviour.
pub enum PingEvent {
    /// We have successfully pinged a peer we are connected to.
    PingSuccess {
        /// Id of the peer that we pinged.
        peer: PeerId,
        /// Time elapsed between when we sent the ping and when we received the response.
        time: Duration,
    }
}

impl<TSubstream> Ping<TSubstream> {
    /// Creates a `Ping`.
    pub fn new() -> Self {
        Ping {
            marker: PhantomData,
            events: Vec::new(),
        }
    }
}

impl<TSubstream> Default for Ping<TSubstream> {
    #[inline]
    fn default() -> Self {
        Ping::new()
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for Ping<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = ProtocolsHandlerSelect<listen_handler::PingListenHandler<TSubstream>, dial_handler::PeriodicPingHandler<TSubstream>>;
    type OutEvent = PingEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        listen_handler::PingListenHandler::new()
            .select(dial_handler::PeriodicPingHandler::new())
    }

    fn inject_connected(&mut self, _: PeerId, _: ConnectedPoint) {}

    fn inject_disconnected(&mut self, _: &PeerId, _: ConnectedPoint) {}

    fn inject_node_event(
        &mut self,
        source: PeerId,
        event: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        if let EitherOutput::Second(dial_handler::OutEvent::PingSuccess(time)) = event {
            self.events.push(PingEvent::PingSuccess {
                peer: source,
                time,
            })
        }
    }

    fn poll(
        &mut self,
        _: &mut PollParameters<TTopology>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if !self.events.is_empty() {
            return Async::Ready(NetworkBehaviourAction::GenerateEvent(self.events.remove(0)));
        }

        Async::NotReady
    }
}
