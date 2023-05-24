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
//! The [`Behaviour`] struct implements the [`NetworkBehaviour`] trait.
//! It will respond to inbound ping requests and periodically send outbound ping requests on every established connection.
//!
//! It is up to the user to implement a health-check / connection management policy based on the ping protocol.
//!
//! For example:
//!
//! - Disconnect from peers with an RTT > 200ms
//! - Disconnect from peers which don't support the ping protocol
//! - Disconnect from peers upon the first ping failure
//!
//! Users should inspect emitted [`Event`] and call APIs on [`Swarm`]:
//!
//! - [`Swarm::close_connection`](libp2p_swarm::Swarm::close_connection) to close a specific connection
//! - [`Swarm::disconnect_peer_id`](libp2p_swarm::Swarm::disconnect_peer_id) to close all connections to a peer
//!
//! [`Swarm`]: libp2p_swarm::Swarm
//! [`Transport`]: libp2p_core::Transport

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod handler;
mod protocol;

use handler::Handler;
use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::FromSwarm, ConnectionDenied, ConnectionId, NetworkBehaviour, PollParameters,
    THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use std::time::Duration;
use std::{
    collections::VecDeque,
    task::{Context, Poll},
};

pub use self::protocol::PROTOCOL_NAME;
pub use handler::{Config, Failure};

/// A [`NetworkBehaviour`] that responds to inbound pings and
/// periodically sends outbound pings on every established connection.
///
/// See the crate root documentation for more information.
pub struct Behaviour {
    /// Configuration for outbound pings.
    config: Config,
    /// Queue of events to yield to the swarm.
    events: VecDeque<Event>,
}

/// Event generated by the `Ping` network behaviour.
#[derive(Debug)]
pub struct Event {
    /// The peer ID of the remote.
    pub peer: PeerId,
    /// The connection the ping was executed on.
    pub connection: ConnectionId,
    /// The result of an inbound or outbound ping.
    pub result: Result<Duration, Failure>,
}

impl Behaviour {
    /// Creates a new `Ping` network behaviour with the given configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            events: VecDeque::new(),
        }
    }
}

impl Default for Behaviour {
    fn default() -> Self {
        Self::new(Config::new())
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(self.config.clone(), peer))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(self.config.clone(), peer))
    }

    fn on_connection_handler_event(
        &mut self,
        peer: PeerId,
        connection: ConnectionId,
        result: THandlerOutEvent<Self>,
    ) {
        self.events.push_front(Event {
            peer,
            connection,
            result,
        })
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(e) = self.events.pop_back() {
            Poll::Ready(ToSwarm::GenerateEvent(e))
        } else {
            Poll::Pending
        }
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        match event {
            FromSwarm::ConnectionEstablished(_)
            | FromSwarm::ConnectionClosed(_)
            | FromSwarm::AddressChange(_)
            | FromSwarm::DialFailure(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListener(_)
            | FromSwarm::NewListenAddr(_)
            | FromSwarm::ExpiredListenAddr(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddr(_)
            | FromSwarm::ExpiredExternalAddr(_) => {}
        }
    }
}
