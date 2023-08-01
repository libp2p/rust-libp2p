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

mod connection_limits;
pub use connection_limits::*;

use libp2p_core::{ConnectedPoint, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionEstablished, DialFailure, ListenFailure},
    dummy, ConnectionClosed, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour,
    PollParameters, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use std::collections::{HashMap, HashSet};
use std::task::{Context, Poll};
use void::Void;

/// Alias type for maintaining backward compatibility.
#[deprecated(note = "Renamed to `StaticConnectionLimits`.")]
pub type ConnectionLimits = StaticConnectionLimits;

/// A [`NetworkBehaviour`] that enforces a set of [`StaticConnectionLimits`].
///
/// For these limits to take effect, this needs to be composed into the behaviour tree of your application.
///
/// If a connection is denied due to a limit, either a [`SwarmEvent::IncomingConnectionError`](libp2p_swarm::SwarmEvent::IncomingConnectionError)
/// or [`SwarmEvent::OutgoingConnectionError`](libp2p_swarm::SwarmEvent::OutgoingConnectionError) will be emitted.
/// The [`ListenError::Denied`](libp2p_swarm::ListenError::Denied) and respectively the [`DialError::Denied`](libp2p_swarm::DialError::Denied) variant
/// contain a [`ConnectionDenied`](libp2p_swarm::ConnectionDenied) type that can be downcast to [`Exceeded`] error if (and only if) **this**
/// behaviour denied the connection.
///
/// If you employ multiple [`NetworkBehaviour`]s that manage connections, it may also be a different error.
///
/// # Example
///
/// ```rust
/// # use libp2p_identify as identify;
/// # use libp2p_ping as ping;
/// # use libp2p_swarm_derive::NetworkBehaviour;
/// # use libp2p_connection_limits as connection_limits;
///
/// #[derive(NetworkBehaviour)]
/// # #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// struct MyBehaviour {
///   identify: identify::Behaviour,
///   ping: ping::Behaviour,
///   limits: connection_limits::Behaviour<connection_limits::StaticConnectionLimits>
/// }
/// ```
// Default type for maintaining backward compatibility.
pub struct Behaviour<C: ConnectionLimitsChecker = StaticConnectionLimits> {
    limits: C,

    pending_inbound_connections: HashSet<ConnectionId>,
    pending_outbound_connections: HashSet<ConnectionId>,
    established_inbound_connections: HashSet<ConnectionId>,
    established_outbound_connections: HashSet<ConnectionId>,
    established_per_peer: HashMap<PeerId, HashSet<ConnectionId>>,
}

impl<C: ConnectionLimitsChecker> Behaviour<C> {
    pub fn new(limits: C) -> Self {
        Self {
            limits,
            pending_inbound_connections: Default::default(),
            pending_outbound_connections: Default::default(),
            established_inbound_connections: Default::default(),
            established_outbound_connections: Default::default(),
            established_per_peer: Default::default(),
        }
    }
}

impl<C: ConnectionLimitsChecker + 'static> NetworkBehaviour for Behaviour<C> {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Void;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.limits.check_limit(
            ConnectionKind::PendingIncoming,
            self.pending_inbound_connections.len(),
        )?;

        self.pending_inbound_connections.insert(connection_id);

        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_inbound_connections.remove(&connection_id);

        self.limits.check_limit(
            ConnectionKind::EstablishedIncoming,
            self.established_inbound_connections.len(),
        )?;
        self.limits.check_limit(
            ConnectionKind::EstablishedPerPeer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
        )?;
        self.limits.check_limit(
            ConnectionKind::EstablishedTotal,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
        )?;

        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.limits.check_limit(
            ConnectionKind::PendingOutgoing,
            self.pending_outbound_connections.len(),
        )?;

        self.pending_outbound_connections.insert(connection_id);

        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_outbound_connections.remove(&connection_id);

        self.limits.check_limit(
            ConnectionKind::EstablishedOutgoing,
            self.established_outbound_connections.len(),
        )?;
        self.limits.check_limit(
            ConnectionKind::EstablishedPerPeer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
        )?;
        self.limits.check_limit(
            ConnectionKind::EstablishedTotal,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
        )?;

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
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                endpoint,
                connection_id,
                ..
            }) => {
                match endpoint {
                    ConnectedPoint::Listener { .. } => {
                        self.established_inbound_connections.insert(connection_id);
                    }
                    ConnectedPoint::Dialer { .. } => {
                        self.established_outbound_connections.insert(connection_id);
                    }
                }

                self.established_per_peer
                    .entry(peer_id)
                    .or_default()
                    .insert(connection_id);
            }
            FromSwarm::DialFailure(DialFailure { connection_id, .. }) => {
                self.pending_outbound_connections.remove(&connection_id);
            }
            FromSwarm::AddressChange(_) => {}
            FromSwarm::ListenFailure(ListenFailure { connection_id, .. }) => {
                self.pending_inbound_connections.remove(&connection_id);
            }
            FromSwarm::NewListener(_) => {}
            FromSwarm::NewListenAddr(_) => {}
            FromSwarm::ExpiredListenAddr(_) => {}
            FromSwarm::ListenerError(_) => {}
            FromSwarm::ListenerClosed(_) => {}
            FromSwarm::NewExternalAddrCandidate(_) => {}
            FromSwarm::ExternalAddrExpired(_) => {}
            FromSwarm::ExternalAddrConfirmed(_) => {}
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
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}
