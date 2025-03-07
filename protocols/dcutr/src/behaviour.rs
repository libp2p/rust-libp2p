// Copyright 2021 Protocol Labs.
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

//! [`NetworkBehaviour`] to act as a direct connection upgrade through relay node.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    num::NonZeroUsize,
    task::{Context, Poll},
};

use either::Either;
use libp2p_core::{
    connection::ConnectedPoint, multiaddr::Protocol, transport::PortUse, Endpoint, Multiaddr,
};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionClosed, DialFailure, FromSwarm},
    dial_opts::{self, DialOpts},
    dummy, ConnectionDenied, ConnectionHandler, ConnectionId, NetworkBehaviour,
    NewExternalAddrCandidate, NotifyHandler, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use lru::LruCache;
use thiserror::Error;

use crate::{handler, protocol};

pub(crate) const MAX_NUMBER_OF_UPGRADE_ATTEMPTS: u8 = 3;

/// The events produced by the [`Behaviour`].
#[derive(Debug)]
pub struct Event {
    pub remote_peer_id: PeerId,
    pub result: Result<ConnectionId, Error>,
}

#[derive(Debug, Error)]
#[error("Failed to hole-punch connection: {inner}")]
pub struct Error {
    inner: InnerError,
}

#[derive(Debug, Error)]
enum InnerError {
    #[error("Giving up after {0} dial attempts")]
    AttemptsExceeded(u8),
    #[error("Inbound stream error: {0}")]
    InboundError(protocol::inbound::Error),
    #[error("Outbound stream error: {0}")]
    OutboundError(protocol::outbound::Error),
}

pub struct Behaviour {
    /// Queue of actions to return when polled.
    queued_events: VecDeque<ToSwarm<Event, Either<handler::relayed::Command, Infallible>>>,

    /// All direct (non-relayed) connections.
    direct_connections: HashMap<PeerId, HashSet<ConnectionId>>,

    address_candidates: Candidates,

    direct_to_relayed_connections: HashMap<ConnectionId, ConnectionId>,

    /// Indexed by the [`ConnectionId`] of the relayed connection and
    /// the [`PeerId`] we are trying to establish a direct connection to.
    outgoing_direct_connection_attempts: HashMap<(ConnectionId, PeerId), u8>,
}

impl Behaviour {
    pub fn new(local_peer_id: PeerId) -> Self {
        Behaviour {
            queued_events: Default::default(),
            direct_connections: Default::default(),
            address_candidates: Candidates::new(local_peer_id),
            direct_to_relayed_connections: Default::default(),
            outgoing_direct_connection_attempts: Default::default(),
        }
    }

    fn observed_addresses(&self) -> Vec<Multiaddr> {
        self.address_candidates.iter().cloned().collect()
    }

    fn on_dial_failure(
        &mut self,
        DialFailure {
            peer_id,
            connection_id: failed_direct_connection,
            ..
        }: DialFailure,
    ) {
        let Some(peer_id) = peer_id else {
            return;
        };

        let Some(relayed_connection_id) = self
            .direct_to_relayed_connections
            .get(&failed_direct_connection)
        else {
            return;
        };

        let Some(attempt) = self
            .outgoing_direct_connection_attempts
            .get(&(*relayed_connection_id, peer_id))
        else {
            return;
        };

        if *attempt < MAX_NUMBER_OF_UPGRADE_ATTEMPTS {
            self.queued_events.push_back(ToSwarm::NotifyHandler {
                handler: NotifyHandler::One(*relayed_connection_id),
                peer_id,
                event: Either::Left(handler::relayed::Command::Connect),
            })
        } else {
            self.queued_events.extend([ToSwarm::GenerateEvent(Event {
                remote_peer_id: peer_id,
                result: Err(Error {
                    inner: InnerError::AttemptsExceeded(MAX_NUMBER_OF_UPGRADE_ATTEMPTS),
                }),
            })]);
        }
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            connection_id,
            endpoint: connected_point,
            ..
        }: ConnectionClosed,
    ) {
        if !connected_point.is_relayed() {
            let connections = self
                .direct_connections
                .get_mut(&peer_id)
                .expect("Peer of direct connection to be tracked.");
            connections
                .remove(&connection_id)
                .then_some(())
                .expect("Direct connection to be tracked.");
            if connections.is_empty() {
                self.direct_connections.remove(&peer_id);
            }
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Either<handler::relayed::Handler, dummy::ConnectionHandler>;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if is_relayed(local_addr) {
            let connected_point = ConnectedPoint::Listener {
                local_addr: local_addr.clone(),
                send_back_addr: remote_addr.clone(),
            };
            let mut handler =
                handler::relayed::Handler::new(connected_point, self.observed_addresses());
            handler.on_behaviour_event(handler::relayed::Command::Connect);

            // TODO: We could make two `handler::relayed::Handler` here, one inbound one outbound.
            return Ok(Either::Left(handler));
        }
        self.direct_connections
            .entry(peer)
            .or_default()
            .insert(connection_id);

        assert!(
            !self
                .direct_to_relayed_connections
                .contains_key(&connection_id),
            "state mismatch"
        );

        Ok(Either::Right(dummy::ConnectionHandler))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if is_relayed(addr) {
            return Ok(Either::Left(handler::relayed::Handler::new(
                ConnectedPoint::Dialer {
                    address: addr.clone(),
                    role_override,
                    port_use,
                },
                self.observed_addresses(),
            ))); // TODO: We could make two `handler::relayed::Handler` here, one inbound one
                 // outbound.
        }

        self.direct_connections
            .entry(peer)
            .or_default()
            .insert(connection_id);

        // Whether this is a connection requested by this behaviour.
        if let Some(&relayed_connection_id) = self.direct_to_relayed_connections.get(&connection_id)
        {
            if role_override == Endpoint::Listener {
                assert!(
                    self.outgoing_direct_connection_attempts
                        .remove(&(relayed_connection_id, peer))
                        .is_some(),
                    "state mismatch"
                );
            }

            self.queued_events.extend([ToSwarm::GenerateEvent(Event {
                remote_peer_id: peer,
                result: Ok(connection_id),
            })]);
        }
        Ok(Either::Right(dummy::ConnectionHandler))
    }

    fn on_connection_handler_event(
        &mut self,
        event_source: PeerId,
        connection_id: ConnectionId,
        handler_event: THandlerOutEvent<Self>,
    ) {
        let relayed_connection_id = match handler_event.as_ref() {
            Either::Left(_) => connection_id,
            Either::Right(_) => match self.direct_to_relayed_connections.get(&connection_id) {
                None => {
                    // If the connection ID is unknown to us, it means we didn't create it so ignore
                    // any event coming from it.
                    return;
                }
                Some(relayed_connection_id) => *relayed_connection_id,
            },
        };

        match handler_event {
            Either::Left(handler::relayed::Event::InboundConnectNegotiated { remote_addrs }) => {
                tracing::debug!(target=%event_source, addresses=?remote_addrs, "Attempting to hole-punch as dialer");

                let opts = DialOpts::peer_id(event_source)
                    .addresses(remote_addrs)
                    .condition(dial_opts::PeerCondition::Always)
                    .build();

                let maybe_direct_connection_id = opts.connection_id();

                self.direct_to_relayed_connections
                    .insert(maybe_direct_connection_id, relayed_connection_id);
                self.queued_events.push_back(ToSwarm::Dial { opts });
            }
            Either::Left(handler::relayed::Event::InboundConnectFailed { error }) => {
                self.queued_events.push_back(ToSwarm::GenerateEvent(Event {
                    remote_peer_id: event_source,
                    result: Err(Error {
                        inner: InnerError::InboundError(error),
                    }),
                }));
            }
            Either::Left(handler::relayed::Event::OutboundConnectFailed { error }) => {
                self.queued_events.push_back(ToSwarm::GenerateEvent(Event {
                    remote_peer_id: event_source,
                    result: Err(Error {
                        inner: InnerError::OutboundError(error),
                    }),
                }));

                // Maybe treat these as transient and retry?
            }
            Either::Left(handler::relayed::Event::OutboundConnectNegotiated { remote_addrs }) => {
                tracing::debug!(target=%event_source, addresses=?remote_addrs, "Attempting to hole-punch as listener");

                let opts = DialOpts::peer_id(event_source)
                    .condition(dial_opts::PeerCondition::Always)
                    .addresses(remote_addrs)
                    .override_role()
                    .build();

                let maybe_direct_connection_id = opts.connection_id();

                self.direct_to_relayed_connections
                    .insert(maybe_direct_connection_id, relayed_connection_id);
                *self
                    .outgoing_direct_connection_attempts
                    .entry((relayed_connection_id, event_source))
                    .or_default() += 1;
                self.queued_events.push_back(ToSwarm::Dial { opts });
            }
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            Either::Right(never) => libp2p_core::util::unreachable(never),
        };
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self))]
    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.on_connection_closed(connection_closed)
            }
            FromSwarm::DialFailure(dial_failure) => self.on_dial_failure(dial_failure),
            FromSwarm::NewExternalAddrCandidate(NewExternalAddrCandidate { addr }) => {
                self.address_candidates.add(addr.clone());
            }
            _ => {}
        }
    }
}

/// Stores our address candidates.
///
/// We use an [`LruCache`] to favor addresses that are reported more often.
/// When attempting a hole-punch, we will try more frequent addresses first.
/// Most of these addresses will come from observations by other nodes (via e.g. the identify
/// protocol). More common observations mean a more likely stable port-mapping and thus a higher
/// chance of a successful hole-punch.
struct Candidates {
    inner: LruCache<Multiaddr, ()>,
    me: PeerId,
}

impl Candidates {
    fn new(me: PeerId) -> Self {
        Self {
            inner: LruCache::new(NonZeroUsize::new(20).expect("20 > 0")),
            me,
        }
    }

    fn add(&mut self, mut address: Multiaddr) {
        if is_relayed(&address) {
            return;
        }

        if address.iter().last() != Some(Protocol::P2p(self.me)) {
            address.push(Protocol::P2p(self.me));
        }

        self.inner.push(address, ());
    }

    fn iter(&self) -> impl Iterator<Item = &Multiaddr> {
        self.inner.iter().map(|(a, _)| a)
    }
}

fn is_relayed(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| p == Protocol::P2pCircuit)
}
