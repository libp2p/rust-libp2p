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

use crate::handler;
use either::Either;
use libp2p_core::connection::ConnectedPoint;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::behaviour::{ConnectionClosed, DialFailure, FromSwarm};
use libp2p_swarm::dial_opts::{self, DialOpts};
use libp2p_swarm::{
    dummy, ConnectionDenied, ConnectionHandler, ConnectionId, THandler, THandlerOutEvent,
};
use libp2p_swarm::{
    ExternalAddresses, NetworkBehaviour, NotifyHandler, PollParameters, StreamUpgradeError,
    THandlerInEvent, ToSwarm,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};
use thiserror::Error;
use void::Void;

pub(crate) const MAX_NUMBER_OF_UPGRADE_ATTEMPTS: u8 = 3;

/// The events produced by the [`Behaviour`].
#[derive(Debug)]
pub enum Event {
    InitiatedDirectConnectionUpgrade {
        remote_peer_id: PeerId,
        local_relayed_addr: Multiaddr,
    },
    RemoteInitiatedDirectConnectionUpgrade {
        remote_peer_id: PeerId,
        remote_relayed_addr: Multiaddr,
    },
    DirectConnectionUpgradeSucceeded {
        remote_peer_id: PeerId,
        connection_id: ConnectionId,
    },
    DirectConnectionUpgradeFailed {
        remote_peer_id: PeerId,
        error: Error,
    },
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to dial peer.")]
    Dial,
    #[error("Failed to establish substream: {0}.")]
    Handler(StreamUpgradeError<Void>),
}

pub struct Behaviour {
    /// Queue of actions to return when polled.
    queued_events: VecDeque<ToSwarm<Event, Either<handler::relayed::Command, Void>>>,

    /// All direct (non-relayed) connections.
    direct_connections: HashMap<PeerId, HashSet<ConnectionId>>,

    external_addresses: ExternalAddresses,

    local_peer_id: PeerId,

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
            external_addresses: Default::default(),
            local_peer_id,
            direct_to_relayed_connections: Default::default(),
            outgoing_direct_connection_attempts: Default::default(),
        }
    }

    fn observed_addresses(&self) -> Vec<Multiaddr> {
        self.external_addresses
            .iter()
            .cloned()
            .filter(|a| !a.iter().any(|p| p == Protocol::P2pCircuit))
            .map(|a| a.with(Protocol::P2p(self.local_peer_id)))
            .collect()
    }

    fn on_dial_failure(
        &mut self,
        DialFailure {
            peer_id,
            connection_id: failed_direct_connection,
            ..
        }: DialFailure,
    ) {
        let peer_id = if let Some(peer_id) = peer_id {
            peer_id
        } else {
            return;
        };

        let relayed_connection_id = if let Some(relayed_connection_id) = self
            .direct_to_relayed_connections
            .get(&failed_direct_connection)
        {
            *relayed_connection_id
        } else {
            return;
        };

        let attempt = if let Some(attempt) = self
            .outgoing_direct_connection_attempts
            .get(&(relayed_connection_id, peer_id))
        {
            *attempt
        } else {
            return;
        };

        if attempt < MAX_NUMBER_OF_UPGRADE_ATTEMPTS {
            self.queued_events.push_back(ToSwarm::NotifyHandler {
                handler: NotifyHandler::One(relayed_connection_id),
                peer_id,
                event: Either::Left(handler::relayed::Command::Connect),
            })
        } else {
            self.queued_events.extend([ToSwarm::GenerateEvent(
                Event::DirectConnectionUpgradeFailed {
                    remote_peer_id: peer_id,
                    error: Error::Dial,
                },
            )]);
        }
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            connection_id,
            endpoint: connected_point,
            ..
        }: ConnectionClosed<<Self as NetworkBehaviour>::ConnectionHandler>,
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

            self.queued_events.extend([ToSwarm::GenerateEvent(
                Event::InitiatedDirectConnectionUpgrade {
                    remote_peer_id: peer,
                    local_relayed_addr: local_addr.clone(),
                },
            )]);

            return Ok(Either::Left(handler)); // TODO: We could make two `handler::relayed::Handler` here, one inbound one outbound.
        }
        self.direct_connections
            .entry(peer)
            .or_default()
            .insert(connection_id);

        assert!(
            self.direct_to_relayed_connections
                .get(&connection_id)
                .is_none(),
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
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if is_relayed(addr) {
            return Ok(Either::Left(handler::relayed::Handler::new(
                ConnectedPoint::Dialer {
                    address: addr.clone(),
                    role_override,
                },
                self.observed_addresses(),
            ))); // TODO: We could make two `handler::relayed::Handler` here, one inbound one outbound.
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

            self.queued_events.extend([ToSwarm::GenerateEvent(
                Event::DirectConnectionUpgradeSucceeded {
                    remote_peer_id: peer,
                    connection_id: relayed_connection_id,
                },
            )]);
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
                    // If the connection ID is unknown to us, it means we didn't create it so ignore any event coming from it.
                    return;
                }
                Some(relayed_connection_id) => *relayed_connection_id,
            },
        };

        match handler_event {
            Either::Left(handler::relayed::Event::InboundConnectRequest { remote_addr }) => {
                self.queued_events.extend([ToSwarm::GenerateEvent(
                    Event::RemoteInitiatedDirectConnectionUpgrade {
                        remote_peer_id: event_source,
                        remote_relayed_addr: remote_addr,
                    },
                )]);
            }
            Either::Left(handler::relayed::Event::InboundNegotiationFailed { error }) => {
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    Event::DirectConnectionUpgradeFailed {
                        remote_peer_id: event_source,
                        error: Error::Handler(error),
                    },
                ));
            }
            Either::Left(handler::relayed::Event::InboundConnectNegotiated(remote_addrs)) => {
                let opts = DialOpts::peer_id(event_source)
                    .addresses(remote_addrs)
                    .condition(dial_opts::PeerCondition::Always)
                    .build();

                let maybe_direct_connection_id = opts.connection_id();

                self.direct_to_relayed_connections
                    .insert(maybe_direct_connection_id, relayed_connection_id);
                self.queued_events.push_back(ToSwarm::Dial { opts });
            }
            Either::Left(handler::relayed::Event::OutboundNegotiationFailed { error }) => {
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    Event::DirectConnectionUpgradeFailed {
                        remote_peer_id: event_source,
                        error: Error::Handler(error),
                    },
                ));
            }
            Either::Left(handler::relayed::Event::OutboundConnectNegotiated { remote_addrs }) => {
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
            Either::Right(never) => void::unreachable(never),
        };
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        self.external_addresses.on_swarm_event(&event);

        match event {
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.on_connection_closed(connection_closed)
            }
            FromSwarm::DialFailure(dial_failure) => self.on_dial_failure(dial_failure),
            FromSwarm::AddressChange(_)
            | FromSwarm::ConnectionEstablished(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListener(_)
            | FromSwarm::NewListenAddr(_)
            | FromSwarm::ExpiredListenAddr(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddrCandidate(_)
            | FromSwarm::ExternalAddrExpired(_)
            | FromSwarm::ExternalAddrConfirmed(_) => {}
        }
    }
}

fn is_relayed(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| p == Protocol::P2pCircuit)
}
