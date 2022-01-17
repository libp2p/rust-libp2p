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
use crate::protocol;
use either::Either;
use libp2p_core::connection::{ConnectedPoint, ConnectionId};
use libp2p_core::multiaddr::Protocol;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::dial_opts::{self, DialOpts};
use libp2p_swarm::{
    DialError, IntoProtocolsHandler, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler,
    PollParameters, ProtocolsHandler, ProtocolsHandlerUpgrErr,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};
use thiserror::Error;

/// The events produced by the [`Behaviour`].
#[derive(Debug)]
pub enum Event {
    InitiateDirectConnectionUpgrade {
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
        error: UpgradeError,
    },
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("Failed to dial peer.")]
    Dial,
    #[error("Failed to establish substream: {0}.")]
    Handler(ProtocolsHandlerUpgrErr<void::Void>),
}

pub struct Behaviour {
    /// Queue of actions to return when polled.
    queued_actions: VecDeque<Action>,

    /// All direct (non-relayed) connections.
    direct_connections: HashMap<PeerId, HashSet<ConnectionId>>,
}

impl Behaviour {
    pub fn new() -> Self {
        Behaviour {
            queued_actions: Default::default(),
            direct_connections: Default::default(),
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ProtocolsHandler = handler::Prototype;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        handler::Prototype::UnknownConnection
    }

    fn addresses_of_peer(&mut self, _peer_id: &PeerId) -> Vec<Multiaddr> {
        vec![]
    }

    fn inject_connected(&mut self, _peer_id: &PeerId) {}

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        connected_point: &ConnectedPoint,
        _failed_addresses: Option<&Vec<Multiaddr>>,
    ) {
        if connected_point.is_relayed() {
            if connected_point.is_listener() && !self.direct_connections.contains_key(peer_id) {
                // TODO: Try dialing the remote peer directly. Specification:
                //
                // > The protocol starts with the completion of a relay connection from A to B. Upon
                // observing the new connection, the inbound peer (here B) checks the addresses
                // advertised by A via identify. If that set includes public addresses, then A may
                // be reachable by a direct connection, in which case B attempts a unilateral
                // connection upgrade by initiating a direct connection to A.
                //
                // https://github.com/libp2p/specs/blob/master/relay/DCUtR.md#the-protocol
                self.queued_actions.push_back(Action::Connect {
                    peer_id: *peer_id,
                    attempt: 1,
                    handler: NotifyHandler::One(*connection_id),
                });
                let local_addr = match connected_point {
                    ConnectedPoint::Listener { local_addr, .. } => local_addr,
                    ConnectedPoint::Dialer { .. } => unreachable!("Due to outer if."),
                };
                self.queued_actions.push_back(
                    NetworkBehaviourAction::GenerateEvent(Event::InitiateDirectConnectionUpgrade {
                        remote_peer_id: *peer_id,
                        local_relayed_addr: local_addr.clone(),
                    })
                    .into(),
                );
            }
        } else {
            self.direct_connections
                .entry(*peer_id)
                .or_default()
                .insert(*connection_id);
        }
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ProtocolsHandler,
        _error: &DialError,
    ) {
        match handler {
            handler::Prototype::DirectConnection {
                relayed_connection_id,
                role: handler::Role::Initiator { attempt },
            } => {
                let peer_id =
                    peer_id.expect("Prototype::DirectConnection is always for known peer.");
                if attempt < 3 {
                    self.queued_actions.push_back(Action::Connect {
                        peer_id,
                        handler: NotifyHandler::One(relayed_connection_id),
                        attempt: attempt + 1,
                    });
                } else {
                    self.queued_actions.push_back(
                        NetworkBehaviourAction::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::One(relayed_connection_id),
                            event: Either::Left(
                                handler::relayed::Command::UpgradeFinishedDontKeepAlive,
                            ),
                        }
                        .into(),
                    );
                    self.queued_actions.push_back(
                        NetworkBehaviourAction::GenerateEvent(
                            Event::DirectConnectionUpgradeFailed {
                                remote_peer_id: peer_id,
                                error: UpgradeError::Dial,
                            },
                        )
                        .into(),
                    );
                }
            }
            _ => {}
        }
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        assert!(!self.direct_connections.contains_key(peer_id));
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        connected_point: &ConnectedPoint,
        _handler: <<Self as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler,
    ) {
        if !connected_point.is_relayed() {
            let connections = self
                .direct_connections
                .get_mut(peer_id)
                .expect("Peer of direct connection to be tracked.");
            connections
                .remove(connection_id)
                .then(|| ())
                .expect("Direct connection to be tracked.");
            if connections.is_empty() {
                self.direct_connections.remove(peer_id);
            }
        }
    }

    fn inject_event(
        &mut self,
        event_source: PeerId,
        connection: ConnectionId,
        handler_event: <<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent,
    ) {
        match handler_event {
            Either::Left(handler::relayed::Event::InboundConnectRequest {
                inbound_connect,
                remote_addr,
            }) => {
                self.queued_actions.push_back(Action::AcceptInboundConnect {
                    peer_id: event_source,
                    handler: NotifyHandler::One(connection),
                    inbound_connect,
                });
                self.queued_actions.push_back(
                    NetworkBehaviourAction::GenerateEvent(
                        Event::RemoteInitiatedDirectConnectionUpgrade {
                            remote_peer_id: event_source,
                            remote_relayed_addr: remote_addr,
                        },
                    )
                    .into(),
                );
            }
            Either::Left(handler::relayed::Event::InboundNegotiationFailed { error }) => {
                self.queued_actions.push_back(
                    NetworkBehaviourAction::GenerateEvent(Event::DirectConnectionUpgradeFailed {
                        remote_peer_id: event_source,
                        error: UpgradeError::Handler(error),
                    })
                    .into(),
                );
            }
            Either::Left(handler::relayed::Event::InboundConnectNegotiated(remote_addrs)) => {
                self.queued_actions.push_back(
                    NetworkBehaviourAction::Dial {
                        opts: DialOpts::peer_id(event_source)
                            .addresses(remote_addrs)
                            .condition(dial_opts::PeerCondition::Always)
                            .build(),
                        handler: handler::Prototype::DirectConnection {
                            relayed_connection_id: connection,
                            role: handler::Role::Listener,
                        },
                    }
                    .into(),
                );
            }
            Either::Left(handler::relayed::Event::OutboundNegotiationFailed { error }) => {
                self.queued_actions.push_back(
                    NetworkBehaviourAction::GenerateEvent(Event::DirectConnectionUpgradeFailed {
                        remote_peer_id: event_source,
                        error: UpgradeError::Handler(error),
                    })
                    .into(),
                );
            }
            Either::Left(handler::relayed::Event::OutboundConnectNegotiated {
                remote_addrs,
                attempt,
            }) => {
                self.queued_actions.push_back(
                    NetworkBehaviourAction::Dial {
                        opts: DialOpts::peer_id(event_source)
                            .condition(dial_opts::PeerCondition::Always)
                            .addresses(remote_addrs)
                            .override_role()
                            .build(),
                        handler: handler::Prototype::DirectConnection {
                            relayed_connection_id: connection,
                            role: handler::Role::Initiator { attempt },
                        },
                    }
                    .into(),
                );
            }
            Either::Right(Either::Left(
                handler::direct::Event::DirectConnectionUpgradeSucceeded {
                    relayed_connection_id,
                },
            )) => {
                self.queued_actions.push_back(
                    NetworkBehaviourAction::NotifyHandler {
                        peer_id: event_source,
                        handler: NotifyHandler::One(relayed_connection_id),
                        event: Either::Left(
                            handler::relayed::Command::UpgradeFinishedDontKeepAlive,
                        ),
                    }
                    .into(),
                );
                self.queued_actions.push_back(
                    NetworkBehaviourAction::GenerateEvent(
                        Event::DirectConnectionUpgradeSucceeded {
                            remote_peer_id: event_source,
                        },
                    )
                    .into(),
                );
                return;
            }
            Either::Right(Either::Right(event)) => void::unreachable(event),
        };
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        poll_parameters: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ProtocolsHandler>> {
        if let Some(action) = self.queued_actions.pop_front() {
            return Poll::Ready(action.build(poll_parameters));
        }

        Poll::Pending
    }
}

/// A [`NetworkBehaviourAction`], either complete, or still requiring data from [`PollParameters`]
/// before being returned in [`Relay::poll`].
#[allow(clippy::large_enum_variant)]
enum Action {
    Done(NetworkBehaviourAction<Event, handler::Prototype>),
    Connect {
        attempt: u8,
        handler: NotifyHandler,
        peer_id: PeerId,
    },
    AcceptInboundConnect {
        inbound_connect: protocol::inbound::PendingConnect,
        handler: NotifyHandler,
        peer_id: PeerId,
    },
}

impl From<NetworkBehaviourAction<Event, handler::Prototype>> for Action {
    fn from(action: NetworkBehaviourAction<Event, handler::Prototype>) -> Self {
        Self::Done(action)
    }
}

impl Action {
    fn build(
        self,
        poll_parameters: &mut impl PollParameters,
    ) -> NetworkBehaviourAction<Event, handler::Prototype> {
        match self {
            Action::Done(action) => action,
            Action::AcceptInboundConnect {
                inbound_connect,
                handler,
                peer_id,
            } => NetworkBehaviourAction::NotifyHandler {
                handler,
                peer_id,
                event: Either::Left(handler::relayed::Command::AcceptInboundConnect {
                    inbound_connect,
                    obs_addrs: poll_parameters
                        .external_addresses()
                        .filter(|a| !a.addr.iter().any(|p| p == Protocol::P2pCircuit))
                        .map(|a| {
                            a.addr
                                .with(Protocol::P2p((*poll_parameters.local_peer_id()).into()))
                        })
                        .collect(),
                }),
            },
            Action::Connect {
                attempt,
                handler,
                peer_id,
            } => NetworkBehaviourAction::NotifyHandler {
                handler,
                peer_id,
                event: Either::Left(handler::relayed::Command::Connect {
                    attempt,
                    obs_addrs: poll_parameters
                        .external_addresses()
                        .filter(|a| !a.addr.iter().any(|p| p == Protocol::P2pCircuit))
                        .map(|a| {
                            a.addr
                                .with(Protocol::P2p((*poll_parameters.local_peer_id()).into()))
                        })
                        .collect(),
                }),
            },
        }
    }
}
