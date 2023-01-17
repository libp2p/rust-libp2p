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
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::behaviour::{ConnectionClosed, ConnectionEstablished, DialFailure, FromSwarm};
use libp2p_swarm::dial_opts::{self, DialOpts};
use libp2p_swarm::ConnectionId;
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerUpgrErr, ExternalAddresses, IntoConnectionHandler,
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};
use thiserror::Error;

const MAX_NUMBER_OF_UPGRADE_ATTEMPTS: u8 = 3;

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
    Handler(ConnectionHandlerUpgrErr<void::Void>),
}

pub struct Behaviour {
    /// Queue of actions to return when polled.
    queued_events: VecDeque<NetworkBehaviourAction<Event, handler::Prototype>>,

    /// All direct (non-relayed) connections.
    direct_connections: HashMap<PeerId, HashSet<ConnectionId>>,

    external_addresses: ExternalAddresses,

    local_peer_id: PeerId,
}

impl Behaviour {
    pub fn new(local_peer_id: PeerId) -> Self {
        Behaviour {
            queued_events: Default::default(),
            direct_connections: Default::default(),
            external_addresses: Default::default(),
            local_peer_id,
        }
    }

    fn observed_addreses(&self) -> Vec<Multiaddr> {
        self.external_addresses
            .iter()
            .cloned()
            .filter(|a| !a.iter().any(|p| p == Protocol::P2pCircuit))
            .map(|a| a.with(Protocol::P2p(self.local_peer_id.into())))
            .collect()
    }

    fn on_connection_established(
        &mut self,
        ConnectionEstablished {
            peer_id,
            connection_id,
            endpoint: connected_point,
            ..
        }: ConnectionEstablished,
    ) {
        if connected_point.is_relayed() {
            if connected_point.is_listener() && !self.direct_connections.contains_key(&peer_id) {
                // TODO: Try dialing the remote peer directly. Specification:
                //
                // > The protocol starts with the completion of a relay connection from A to B. Upon
                // observing the new connection, the inbound peer (here B) checks the addresses
                // advertised by A via identify. If that set includes public addresses, then A may
                // be reachable by a direct connection, in which case B attempts a unilateral
                // connection upgrade by initiating a direct connection to A.
                //
                // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
                self.queued_events.extend([
                    NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler: NotifyHandler::One(connection_id),
                        event: Either::Left(handler::relayed::Command::Connect {
                            obs_addrs: self.observed_addreses(),
                            attempt: 1,
                        }),
                    },
                    NetworkBehaviourAction::GenerateEvent(
                        Event::InitiatedDirectConnectionUpgrade {
                            remote_peer_id: peer_id,
                            local_relayed_addr: match connected_point {
                                ConnectedPoint::Listener { local_addr, .. } => local_addr.clone(),
                                ConnectedPoint::Dialer { .. } => unreachable!("Due to outer if."),
                            },
                        },
                    ),
                ]);
            }
        } else {
            self.direct_connections
                .entry(peer_id)
                .or_default()
                .insert(connection_id);
        }
    }

    fn on_dial_failure(
        &mut self,
        DialFailure {
            peer_id, handler, ..
        }: DialFailure<<Self as NetworkBehaviour>::ConnectionHandler>,
    ) {
        if let handler::Prototype::DirectConnection {
            relayed_connection_id,
            role: handler::Role::Initiator { attempt },
        } = handler
        {
            let peer_id = peer_id.expect("Peer of `Prototype::DirectConnection` is always known.");
            if attempt < MAX_NUMBER_OF_UPGRADE_ATTEMPTS {
                self.queued_events
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        handler: NotifyHandler::One(relayed_connection_id),
                        peer_id,
                        event: Either::Left(handler::relayed::Command::Connect {
                            attempt: attempt + 1,
                            obs_addrs: self.observed_addreses(),
                        }),
                    })
            } else {
                self.queued_events.extend([
                    NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler: NotifyHandler::One(relayed_connection_id),
                        event: Either::Left(
                            handler::relayed::Command::UpgradeFinishedDontKeepAlive,
                        ),
                    },
                    NetworkBehaviourAction::GenerateEvent(Event::DirectConnectionUpgradeFailed {
                        remote_peer_id: peer_id,
                        error: Error::Dial,
                    }),
                ]);
            }
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
    type ConnectionHandler = handler::Prototype;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        handler::Prototype::UnknownConnection
    }

    fn on_connection_handler_event(
        &mut self,
        event_source: PeerId,
        connection: ConnectionId,
        handler_event: <<Self::ConnectionHandler as IntoConnectionHandler>::Handler as
            ConnectionHandler>::OutEvent,
    ) {
        match handler_event {
            Either::Left(handler::relayed::Event::InboundConnectRequest {
                inbound_connect,
                remote_addr,
            }) => {
                self.queued_events.extend([
                    NetworkBehaviourAction::NotifyHandler {
                        handler: NotifyHandler::One(connection),
                        peer_id: event_source,
                        event: Either::Left(handler::relayed::Command::AcceptInboundConnect {
                            inbound_connect,
                            obs_addrs: self.observed_addreses(),
                        }),
                    },
                    NetworkBehaviourAction::GenerateEvent(
                        Event::RemoteInitiatedDirectConnectionUpgrade {
                            remote_peer_id: event_source,
                            remote_relayed_addr: remote_addr,
                        },
                    ),
                ]);
            }
            Either::Left(handler::relayed::Event::InboundNegotiationFailed { error }) => {
                self.queued_events
                    .push_back(NetworkBehaviourAction::GenerateEvent(
                        Event::DirectConnectionUpgradeFailed {
                            remote_peer_id: event_source,
                            error: Error::Handler(error),
                        },
                    ));
            }
            Either::Left(handler::relayed::Event::InboundConnectNegotiated(remote_addrs)) => {
                self.queued_events.push_back(NetworkBehaviourAction::Dial {
                    opts: DialOpts::peer_id(event_source)
                        .addresses(remote_addrs)
                        .condition(dial_opts::PeerCondition::Always)
                        .build(),
                    handler: handler::Prototype::DirectConnection {
                        relayed_connection_id: connection,
                        role: handler::Role::Listener,
                    },
                });
            }
            Either::Left(handler::relayed::Event::OutboundNegotiationFailed { error }) => {
                self.queued_events
                    .push_back(NetworkBehaviourAction::GenerateEvent(
                        Event::DirectConnectionUpgradeFailed {
                            remote_peer_id: event_source,
                            error: Error::Handler(error),
                        },
                    ));
            }
            Either::Left(handler::relayed::Event::OutboundConnectNegotiated {
                remote_addrs,
                attempt,
            }) => {
                self.queued_events.push_back(NetworkBehaviourAction::Dial {
                    opts: DialOpts::peer_id(event_source)
                        .condition(dial_opts::PeerCondition::Always)
                        .addresses(remote_addrs)
                        .override_role()
                        .build(),
                    handler: handler::Prototype::DirectConnection {
                        relayed_connection_id: connection,
                        role: handler::Role::Initiator { attempt },
                    },
                });
            }
            Either::Right(Either::Left(
                handler::direct::Event::DirectConnectionUpgradeSucceeded {
                    relayed_connection_id,
                },
            )) => {
                self.queued_events.extend([
                    NetworkBehaviourAction::NotifyHandler {
                        peer_id: event_source,
                        handler: NotifyHandler::One(relayed_connection_id),
                        event: Either::Left(
                            handler::relayed::Command::UpgradeFinishedDontKeepAlive,
                        ),
                    },
                    NetworkBehaviourAction::GenerateEvent(
                        Event::DirectConnectionUpgradeSucceeded {
                            remote_peer_id: event_source,
                        },
                    ),
                ]);
            }
            Either::Right(Either::Right(event)) => void::unreachable(event),
        };
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        self.external_addresses.on_swarm_event(&event);

        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                self.on_connection_established(connection_established)
            }
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.on_connection_closed(connection_closed)
            }
            FromSwarm::DialFailure(dial_failure) => self.on_dial_failure(dial_failure),
            FromSwarm::AddressChange(_)
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
