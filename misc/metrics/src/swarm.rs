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

use crate::protocol_stack;
use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};
use prometheus_client::registry::Registry;

pub struct Metrics {
    connections_incoming: Family<AddressLabels, Counter>,
    connections_incoming_error: Family<IncomingConnectionErrorLabels, Counter>,

    connections_established: Family<ConnectionEstablishedLabels, Counter>,
    connections_establishment_duration: Family<ConnectionEstablishmentDurationLabels, Histogram>,
    connections_closed: Family<ConnectionClosedLabels, Counter>,

    new_listen_addr: Family<AddressLabels, Counter>,
    expired_listen_addr: Family<AddressLabels, Counter>,

    listener_closed: Family<AddressLabels, Counter>,
    listener_error: Counter,

    dial_attempt: Counter,
    outgoing_connection_error: Family<OutgoingConnectionErrorLabels, Counter>,
    connected_to_banned_peer: Family<AddressLabels, Counter>,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
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

        let connected_to_banned_peer = Family::default();
        sub_registry.register(
            "connected_to_banned_peer",
            "Number of connection attempts to banned peer",
            connected_to_banned_peer.clone(),
        );

        let connections_established = Family::default();
        sub_registry.register(
            "connections_established",
            "Number of connections established",
            connections_established.clone(),
        );

        let connections_closed = Family::default();
        sub_registry.register(
            "connections_closed",
            "Number of connections closed",
            connections_closed.clone(),
        );

        let connections_establishment_duration = Family::new_with_constructor(
            create_connection_establishment_duration_histogram as fn() -> Histogram,
        );
        sub_registry.register(
            "connections_establishment_duration",
            "Time it took (locally) to establish connections",
            connections_establishment_duration.clone(),
        );

        Self {
            connections_incoming,
            connections_incoming_error,
            connections_established,
            connections_closed,
            new_listen_addr,
            expired_listen_addr,
            listener_closed,
            listener_error,
            dial_attempt,
            outgoing_connection_error,
            connected_to_banned_peer,
            connections_establishment_duration,
        }
    }
}

impl<TBvEv, THandleErr> super::Recorder<libp2p_swarm::SwarmEvent<TBvEv, THandleErr>> for Metrics {
    fn record(&self, event: &libp2p_swarm::SwarmEvent<TBvEv, THandleErr>) {
        match event {
            libp2p_swarm::SwarmEvent::Behaviour(_) => {}
            libp2p_swarm::SwarmEvent::ConnectionEstablished {
                endpoint,
                established_in: time_taken,
                ..
            } => {
                let labels = ConnectionEstablishedLabels {
                    role: endpoint.into(),
                    protocols: protocol_stack::as_string(endpoint.get_remote_address()),
                };
                self.connections_established.get_or_create(&labels).inc();
                self.connections_establishment_duration
                    .get_or_create(&labels)
                    .observe(time_taken.as_secs_f64());
            }
            libp2p_swarm::SwarmEvent::ConnectionClosed { endpoint, .. } => {
                self.connections_closed
                    .get_or_create(&ConnectionClosedLabels {
                        role: endpoint.into(),
                        protocols: protocol_stack::as_string(endpoint.get_remote_address()),
                    })
                    .inc();
            }
            libp2p_swarm::SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                self.connections_incoming
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(send_back_addr),
                    })
                    .inc();
            }
            libp2p_swarm::SwarmEvent::IncomingConnectionError {
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
            libp2p_swarm::SwarmEvent::OutgoingConnectionError { error, peer_id } => {
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
                    libp2p_swarm::DialError::Transport(errors) => {
                        for (_multiaddr, error) in errors {
                            match error {
                                libp2p_core::transport::TransportError::MultiaddrNotSupported(
                                    _,
                                ) => record(
                                    OutgoingConnectionErrorError::TransportMultiaddrNotSupported,
                                ),
                                libp2p_core::transport::TransportError::Other(_) => {
                                    record(OutgoingConnectionErrorError::TransportOther)
                                }
                            };
                        }
                    }

                    libp2p_swarm::DialError::Banned => record(OutgoingConnectionErrorError::Banned),
                    libp2p_swarm::DialError::ConnectionLimit(_) => {
                        record(OutgoingConnectionErrorError::ConnectionLimit)
                    }
                    libp2p_swarm::DialError::LocalPeerId { .. } => {
                        record(OutgoingConnectionErrorError::LocalPeerId)
                    }
                    libp2p_swarm::DialError::NoAddresses => {
                        record(OutgoingConnectionErrorError::NoAddresses)
                    }
                    libp2p_swarm::DialError::DialPeerConditionFalse(_) => {
                        record(OutgoingConnectionErrorError::DialPeerConditionFalse)
                    }
                    libp2p_swarm::DialError::Aborted => {
                        record(OutgoingConnectionErrorError::Aborted)
                    }
                    libp2p_swarm::DialError::InvalidPeerId { .. } => {
                        record(OutgoingConnectionErrorError::InvalidPeerId)
                    }
                    libp2p_swarm::DialError::WrongPeerId { .. } => {
                        record(OutgoingConnectionErrorError::WrongPeerId)
                    }
                };
            }
            libp2p_swarm::SwarmEvent::BannedPeer { endpoint, .. } => {
                self.connected_to_banned_peer
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(endpoint.get_remote_address()),
                    })
                    .inc();
            }
            libp2p_swarm::SwarmEvent::NewListenAddr { address, .. } => {
                self.new_listen_addr
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(address),
                    })
                    .inc();
            }
            libp2p_swarm::SwarmEvent::ExpiredListenAddr { address, .. } => {
                self.expired_listen_addr
                    .get_or_create(&AddressLabels {
                        protocols: protocol_stack::as_string(address),
                    })
                    .inc();
            }
            libp2p_swarm::SwarmEvent::ListenerClosed { addresses, .. } => {
                for address in addresses {
                    self.listener_closed
                        .get_or_create(&AddressLabels {
                            protocols: protocol_stack::as_string(address),
                        })
                        .inc();
                }
            }
            libp2p_swarm::SwarmEvent::ListenerError { .. } => {
                self.listener_error.inc();
            }
            libp2p_swarm::SwarmEvent::Dialing(_) => {
                self.dial_attempt.inc();
            }
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct ConnectionEstablishedLabels {
    role: Role,
    protocols: String,
}

type ConnectionEstablishmentDurationLabels = ConnectionEstablishedLabels;

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct ConnectionClosedLabels {
    role: Role,
    protocols: String,
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
    error: OutgoingConnectionErrorError,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Copy, Debug)]
enum PeerStatus {
    Known,
    Unknown,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum OutgoingConnectionErrorError {
    Banned,
    ConnectionLimit,
    LocalPeerId,
    NoAddresses,
    DialPeerConditionFalse,
    Aborted,
    InvalidPeerId,
    WrongPeerId,
    TransportMultiaddrNotSupported,
    TransportOther,
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct IncomingConnectionErrorLabels {
    error: PendingInboundConnectionError,
    protocols: String,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum PendingInboundConnectionError {
    WrongPeerId,
    LocalPeerId,
    TransportErrorMultiaddrNotSupported,
    TransportErrorOther,
    Aborted,
    ConnectionLimit,
}

impl From<&libp2p_swarm::PendingInboundConnectionError> for PendingInboundConnectionError {
    fn from(error: &libp2p_swarm::PendingInboundConnectionError) -> Self {
        match error {
            libp2p_swarm::PendingInboundConnectionError::WrongPeerId { .. } => {
                PendingInboundConnectionError::WrongPeerId
            }
            libp2p_swarm::PendingInboundConnectionError::LocalPeerId { .. } => {
                PendingInboundConnectionError::LocalPeerId
            }
            libp2p_swarm::PendingInboundConnectionError::ConnectionLimit(_) => {
                PendingInboundConnectionError::ConnectionLimit
            }
            libp2p_swarm::PendingInboundConnectionError::Transport(
                libp2p_core::transport::TransportError::MultiaddrNotSupported(_),
            ) => PendingInboundConnectionError::TransportErrorMultiaddrNotSupported,
            libp2p_swarm::PendingInboundConnectionError::Transport(
                libp2p_core::transport::TransportError::Other(_),
            ) => PendingInboundConnectionError::TransportErrorOther,
            libp2p_swarm::PendingInboundConnectionError::Aborted => {
                PendingInboundConnectionError::Aborted
            }
        }
    }
}

fn create_connection_establishment_duration_histogram() -> Histogram {
    Histogram::new(exponential_buckets(0.01, 1.5, 20))
}
