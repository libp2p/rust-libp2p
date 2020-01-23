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

//! This module implements the `/ipfs/ping/1.0.0` protocol.
//!
//! The ping protocol can be used as a simple application-layer health check
//! for connections of any [`Transport`] as well as to measure and record
//! round-trip times.
//!
//! # Usage
//!
//! The [`Ping`] struct implements the [`NetworkBehaviour`] trait. When used with a [`Swarm`],
//! it will respond to inbound ping requests and as necessary periodically send outbound
//! ping requests on every established connection. If a configurable number of pings fail,
//! the connection will be closed.
//!
//! The `Ping` network behaviour produces [`PingEvent`]s, which may be consumed from the `Swarm`
//! by an application, e.g. to collect statistics.
//!
//! > **Note**: The ping protocol does not keep otherwise idle connections alive,
//! > it only adds an additional condition for terminating the connection, namely
//! > a certain number of failed ping requests.
//!
//! [`Swarm`]: libp2p_core::Swarm
//! [`Transport`]: libp2p_core::Transport

pub mod protocol;
pub mod handler;

pub use handler::{PingConfig, PingResult, PingSuccess, PingFailure};
use handler::PingHandler;

use libp2p_core::{ConnectedPoint, Multiaddr, PeerId};
use libp2p_swarm::{NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use std::{collections::VecDeque, task::Context, task::Poll};
use void::Void;

/// `Ping` is a [`NetworkBehaviour`] that responds to inbound pings and
/// periodically sends outbound pings on every established connection.
///
/// See the crate root documentation for more information.
pub struct Ping {
    /// Configuration for outbound pings.
    config: PingConfig,
    /// Queue of events to yield to the swarm.
    events: VecDeque<PingEvent>,
}

/// Event generated by the `Ping` network behaviour.
#[derive(Debug)]
pub struct PingEvent {
    /// The peer ID of the remote.
    pub peer: PeerId,
    /// The result of an inbound or outbound ping.
    pub result: PingResult,
}

impl Ping {
    /// Creates a new `Ping` network behaviour with the given configuration.
    pub fn new(config: PingConfig) -> Self {
        Ping {
            config,
            events: VecDeque::new(),
        }
    }
}

impl Default for Ping {
    fn default() -> Self {
        Ping::new(PingConfig::new())
    }
}

impl NetworkBehaviour for Ping {
    type ProtocolsHandler = PingHandler;
    type OutEvent = PingEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        PingHandler::new(self.config.clone())
    }

    fn addresses_of_peer(&mut self, _peer_id: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: PeerId, _: ConnectedPoint) {}

    fn inject_disconnected(&mut self, _: &PeerId, _: ConnectedPoint) {}

    fn inject_node_event(&mut self, peer: PeerId, result: PingResult) {
        self.events.push_front(PingEvent { peer, result })
    }

    fn poll(&mut self, _: &mut Context, _: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<Void, PingEvent>>
    {
        if let Some(e) = self.events.pop_back() {
            Poll::Ready(NetworkBehaviourAction::GenerateEvent(e))
        } else {
            Poll::Pending
        }
    }
}
