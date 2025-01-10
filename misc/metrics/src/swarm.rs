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

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use libp2p_swarm::{ConnectionId, DialError, SwarmEvent};
use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{
        counter::Counter,
        family::Family,
        histogram::{exponential_buckets, Histogram},
    },
    registry::{Registry, Unit},
};
use web_time::Instant;

use crate::protocol_stack;

pub(crate) struct Metrics {
    connections_incoming: Family<AddressLabels, Counter>,
    connections_incoming_error: Family<IncomingConnectionErrorLabels, Counter>,

    connections_established: Family<ConnectionLabels, Counter>,
    connections_establishment_duration: Family<ConnectionLabels, Histogram>,
    connections_duration: Family<ConnectionClosedLabels, Histogram>,

    new_listen_addr: Family<AddressLabels, Counter>,
    expired_listen_addr: Family<AddressLabels, Counter>,

    external_addr_candidates: Family<AddressLabels, Counter>,
    external_addr_confirmed: Family<AddressLabels, Counter>,
    external_addr_expired: Family<AddressLabels, Counter>,

    listener_closed: Family<AddressLabels, Counter>,
    listener_error: Counter,

    dial_attempt: Counter,
    outgoing_connection_error: Family<OutgoingConnectionErrorLabels, Counter>,

    connections: Arc<Mutex<HashMap<ConnectionId, Instant>>>,
}

impl Metrics {
    pub(crate) fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("swarm");

        let connections_incoming = Family::default();
        sub_registry.register(
            "connections_incoming",
            "Number of incoming connections per address stack",
            connections_incoming.clone(),
        );

        let connections_incoming_error = Family::default();
        sub_registry.register(
            "connections_incoming_error",
            "Number of incoming connection errors",
            connections_incoming_error.clone(),
        );

        let new_listen_addr = Family::default();
        sub_registry.register(
            "new_listen_addr",
            "Number of new listen addresses",
            new_listen_addr.clone(),
        );

        let expired_listen_addr = Family::default();
        sub_registry.register(
            "expired_listen_addr",
            "Number of expired listen addresses",
            expired_listen_addr.clone(),
        );

        let external_addr_candidates = Family::default();
        sub_registry.register(
            "external_addr_candidates",
            "Number of new external address candidates",
            external_addr_candidates.clone(),
        );

        let external_addr_confirmed = Family::default();
        sub_registry.register(
            "external_addr_confirmed",
            "Number of confirmed external addresses",
            external_addr_confirmed.clone(),
        );

        let external_addr_expired = Family::default();
        sub_registry.register(
            "external_addr_expired",
            "Number of expired external addresses",
            external_addr_expired.clone(),
        );

        let listener_closed = Family::default();
        sub_registry.register(
            "listener_closed",
            "Number of listeners closed",
            listener_closed.clone(),
        );

        let listener_error = Counter::default();
        sub_registry.register(
            "listener_error",
            "Number of listener errors",
            listener_error.clone(),
        );

        let dial_attempt = Counter::default();
        sub_registry.register(
            "dial_attempt",
            "Number of dial attempts",
            dial_attempt.clone(),
        );

        let outgoing_connection_error = Family::default();
        sub_registry.register(
            "outgoing_connection_error",
            "Number outgoing connection errors",
            outgoing_connection_error.clone(),
        );

        let connections_established = Family::default();
        sub_registry.register(
            "connections_established",
            "Number of connections established",
            connections_established.clone(),
        );

        let connections_establishment_duration = {
            let constructor: fn() -> Histogram =
                || Histogram::new(exponential_buckets(0.01, 1.5, 20));
            Family::new_with_constructor(constructor)
        };
        sub_registry.register(
            "connections_establishment_duration",
            "Time it took (locally) to establish connections",
            connections_establishment_duration.clone(),
        );

        let connections_duration = {
            let constructor: fn() -> Histogram =
                || Histogram::new(exponential_buckets(0.01, 3.0, 20));
            Family::new_with_constructor(constructor)
        };
        sub_registry.register_with_unit(
            "connections_duration",
            "Time a connection was alive",
            Unit::Seconds,
            connections_duration.clone(),
        );

        Self {
            connections_incoming,
            connections_incoming_error,
            connections_established,
            new_listen_addr,
            expired_listen_addr,
            external_addr_candidates,
            external_addr_confirmed,
            external_addr_expired,
            listener_closed,
            listener_error,
            dial_attempt,
            outgoing_connection_error,
            connections_establishment_duration,
            connections_duration,
            connections: Default::default(),
        }
    }
}

impl<TBvEv> super::Recorder<SwarmEvent<TBvEv>> for Metrics {
    fn record(&self, event: &SwarmEvent<TBvEv>) {
        match event {
            SwarmEvent::Behaviour(_) => {}
            SwarmEvent::ConnectionEstablished {
                endpoint,
                established_in: time_taken,
                connection_id,
                ..
            } => {
                let labels = ConnectionLabels {
                    role: endpoint.into(),
                    protocols: protocol_stack::as_string(endpoint.get_remote_address()),
                };
                self.connections_established.get_or_create(&labels).inc();
                self.connections_establishment_duration
                    .get_or_create(&labels)
                    .observe(time_taken.as_secs_f64());
                self.connections
                    .lock()
                    .expect("lock not to be poisoned")
                    .insert(*connection_id, Instant::now());
            }
            SwarmEvent::ConnectionClosed {
                endpoint,
                connection_id,
                cause,
                ..
            } => {
                let labels = ConnectionClosedLabels {
                    connection: ConnectionLabels {
                        role: endpoint.into(),
                        protocols: protocol_stack::as_string(endpoint.get_remote_address()),
                    },
                    cause: cause.as_ref().map(Into::into),
                };
                self.connections_duration.get_or_create(&labels).observe(
                    self.connections
                        .lock()
                        .expect("lock not to be poisoned")
                        .remove(connection_id)
                        .expect("closed connection to previously be established")
                        .elapsed()
                        .as_secs_f64(),
                );
            }
            SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                self.connections_incoming
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(send_back_addr),
                    })
                    .inc();
            }
            SwarmEvent::IncomingConnectionError {
                error,
                send_back_addr,
                ..
            } => {
                self.connections_incoming_error
                    .get_or_create(&IncomingConnectionErrorLabels {
                        error: error.into(),
                        protocols: protocol_stack::as_string(send_back_addr),
                    })
                    .inc();
            }
            SwarmEvent::OutgoingConnectionError { error, peer_id, .. } => {
                let peer = match peer_id {
                    Some(_) => PeerStatus::Known,
                    None => PeerStatus::Unknown,
                };

                let record = |error| {
                    self.outgoing_connection_error
                        .get_or_create(&OutgoingConnectionErrorLabels { peer, error })
                        .inc();
                };

                match error {
                    DialError::Transport(errors) => {
                        for (_multiaddr, error) in errors {
                            match error {
                                libp2p_core::transport::TransportError::MultiaddrNotSupported(
                                    _,
                                ) => {
                                    record(OutgoingConnectionError::TransportMultiaddrNotSupported)
                                }
                                libp2p_core::transport::TransportError::Other(_) => {
                                    record(OutgoingConnectionError::TransportOther)
                                }
                            };
                        }
                    }
                    DialError::LocalPeerId { .. } => record(OutgoingConnectionError::LocalPeerId),
                    DialError::NoAddresses => record(OutgoingConnectionError::NoAddresses),
                    DialError::DialPeerConditionFalse(_) => {
                        record(OutgoingConnectionError::DialPeerConditionFalse)
                    }
                    DialError::Aborted => record(OutgoingConnectionError::Aborted),
                    DialError::WrongPeerId { .. } => record(OutgoingConnectionError::WrongPeerId),
                    DialError::Denied { .. } => record(OutgoingConnectionError::Denied),
                };
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                self.new_listen_addr
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(address),
                    })
                    .inc();
            }
            SwarmEvent::ExpiredListenAddr { address, .. } => {
                self.expired_listen_addr
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(address),
                    })
                    .inc();
            }
            SwarmEvent::ListenerClosed { addresses, .. } => {
                for address in addresses {
                    self.listener_closed
                        .get_or_create(&AddressLabels {
                            protocols: protocol_stack::as_string(address),
                        })
                        .inc();
                }
            }
            SwarmEvent::ListenerError { .. } => {
                self.listener_error.inc();
            }
            SwarmEvent::Dialing { .. } => {
                self.dial_attempt.inc();
            }
            SwarmEvent::NewExternalAddrCandidate { address } => {
                self.external_addr_candidates
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(address),
                    })
                    .inc();
            }
            SwarmEvent::ExternalAddrConfirmed { address } => {
                self.external_addr_confirmed
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(address),
                    })
                    .inc();
            }
            SwarmEvent::ExternalAddrExpired { address } => {
                self.external_addr_expired
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(address),
                    })
                    .inc();
            }
            _ => {}
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct ConnectionLabels {
    role: Role,
    protocols: String,
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct ConnectionClosedLabels {
    cause: Option<ConnectionError>,
    #[prometheus(flatten)]
    connection: ConnectionLabels,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum ConnectionError {
    Io,
    KeepAliveTimeout,
}

impl From<&libp2p_swarm::ConnectionError> for ConnectionError {
    fn from(value: &libp2p_swarm::ConnectionError) -> Self {
        match value {
            libp2p_swarm::ConnectionError::IO(_) => ConnectionError::Io,
            libp2p_swarm::ConnectionError::KeepAliveTimeout => ConnectionError::KeepAliveTimeout,
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct AddressLabels {
    protocols: String,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum Role {
    Dialer,
    Listener,
}

impl From<&libp2p_core::ConnectedPoint> for Role {
    fn from(point: &libp2p_core::ConnectedPoint) -> Self {
        match point {
            libp2p_core::ConnectedPoint::Dialer { .. } => Role::Dialer,
            libp2p_core::ConnectedPoint::Listener { .. } => Role::Listener,
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct OutgoingConnectionErrorLabels {
    peer: PeerStatus,
    error: OutgoingConnectionError,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Copy, Debug)]
enum PeerStatus {
    Known,
    Unknown,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum OutgoingConnectionError {
    LocalPeerId,
    NoAddresses,
    DialPeerConditionFalse,
    Aborted,
    WrongPeerId,
    TransportMultiaddrNotSupported,
    TransportOther,
    Denied,
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct IncomingConnectionErrorLabels {
    error: IncomingConnectionError,
    protocols: String,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum IncomingConnectionError {
    WrongPeerId,
    LocalPeerId,
    TransportErrorMultiaddrNotSupported,
    TransportErrorOther,
    Aborted,
    Denied,
}

impl From<&libp2p_swarm::ListenError> for IncomingConnectionError {
    fn from(error: &libp2p_swarm::ListenError) -> Self {
        match error {
            libp2p_swarm::ListenError::WrongPeerId { .. } => IncomingConnectionError::WrongPeerId,
            libp2p_swarm::ListenError::LocalPeerId { .. } => IncomingConnectionError::LocalPeerId,
            libp2p_swarm::ListenError::Transport(
                libp2p_core::transport::TransportError::MultiaddrNotSupported(_),
            ) => IncomingConnectionError::TransportErrorMultiaddrNotSupported,
            libp2p_swarm::ListenError::Transport(
                libp2p_core::transport::TransportError::Other(_),
            ) => IncomingConnectionError::TransportErrorOther,
            libp2p_swarm::ListenError::Aborted => IncomingConnectionError::Aborted,
            libp2p_swarm::ListenError::Denied { .. } => IncomingConnectionError::Denied,
        }
    }
}
