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

use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;

pub struct Metrics {
    connections_incoming: Counter,
    connections_incoming_error: Family<IncomingConnectionErrorLabels, Counter>,

    connections_established: Family<ConnectionEstablishedLabels, Counter>,
    connections_closed: Family<ConnectionClosedLabels, Counter>,

    new_listen_addr: Counter,
    expired_listen_addr: Counter,

    listener_closed: Counter,
    listener_error: Counter,

    dial_attempt: Counter,
    outgoing_connection_error: Family<OutgoingConnectionErrorLabels, Counter>,
    connected_to_banned_peer: Counter,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("swarm");

        let connections_incoming = Counter::default();
        sub_registry.register(
            "connections_incoming",
            "Number of incoming connections",
            Box::new(connections_incoming.clone()),
        );

        let connections_incoming_error = Family::default();
        sub_registry.register(
            "connections_incoming_error",
            "Number of incoming connection errors",
            Box::new(connections_incoming_error.clone()),
        );

        let new_listen_addr = Counter::default();
        sub_registry.register(
            "new_listen_addr",
            "Number of new listen addresses",
            Box::new(new_listen_addr.clone()),
        );

        let expired_listen_addr = Counter::default();
        sub_registry.register(
            "expired_listen_addr",
            "Number of expired listen addresses",
            Box::new(expired_listen_addr.clone()),
        );

        let listener_closed = Counter::default();
        sub_registry.register(
            "listener_closed",
            "Number of listeners closed",
            Box::new(listener_closed.clone()),
        );

        let listener_error = Counter::default();
        sub_registry.register(
            "listener_error",
            "Number of listener errors",
            Box::new(listener_error.clone()),
        );

        let dial_attempt = Counter::default();
        sub_registry.register(
            "dial_attempt",
            "Number of dial attempts",
            Box::new(dial_attempt.clone()),
        );

        let outgoing_connection_error = Family::default();
        sub_registry.register(
            "outgoing_connection_error",
            "Number outgoing connection errors",
            Box::new(outgoing_connection_error.clone()),
        );

        let connected_to_banned_peer = Counter::default();
        sub_registry.register(
            "connected_to_banned_peer",
            "Number of connection attempts to banned peer",
            Box::new(connected_to_banned_peer.clone()),
        );

        let connections_established = Family::default();
        sub_registry.register(
            "connections_established",
            "Number of connections established",
            Box::new(connections_established.clone()),
        );

        let connections_closed = Family::default();
        sub_registry.register(
            "connections_closed",
            "Number of connections closed",
            Box::new(connections_closed.clone()),
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
        }
    }
}

impl<TBvEv, THandleErr> super::Recorder<libp2p_swarm::SwarmEvent<TBvEv, THandleErr>> for Metrics {
    fn record(&self, event: &libp2p_swarm::SwarmEvent<TBvEv, THandleErr>) {
        match event {
            libp2p_swarm::SwarmEvent::Behaviour(_) => {}
            libp2p_swarm::SwarmEvent::ConnectionEstablished { endpoint, .. } => {
                self.connections_established
                    .get_or_create(&ConnectionEstablishedLabels {
                        role: endpoint.into(),
                    })
                    .inc();
            }
            libp2p_swarm::SwarmEvent::ConnectionClosed { endpoint, .. } => {
                self.connections_closed
                    .get_or_create(&ConnectionClosedLabels {
                        role: endpoint.into(),
                    })
                    .inc();
            }
            libp2p_swarm::SwarmEvent::IncomingConnection { .. } => {
                self.connections_incoming.inc();
            }
            libp2p_swarm::SwarmEvent::IncomingConnectionError { error, .. } => {
                self.connections_incoming_error
                    .get_or_create(&IncomingConnectionErrorLabels {
                        error: error.into(),
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
                    libp2p_swarm::DialError::LocalPeerId => {
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
                    libp2p_swarm::DialError::ConnectionIo(_) => {
                        record(OutgoingConnectionErrorError::ConnectionIo)
                    }
                };
            }
            libp2p_swarm::SwarmEvent::BannedPeer { .. } => {
                self.connected_to_banned_peer.inc();
            }
            libp2p_swarm::SwarmEvent::NewListenAddr { .. } => {
                self.new_listen_addr.inc();
            }
            libp2p_swarm::SwarmEvent::ExpiredListenAddr { .. } => {
                self.expired_listen_addr.inc();
            }
            libp2p_swarm::SwarmEvent::ListenerClosed { .. } => {
                self.listener_closed.inc();
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

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
struct ConnectionEstablishedLabels {
    role: Role,
}

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
struct ConnectionClosedLabels {
    role: Role,
}

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
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

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
struct OutgoingConnectionErrorLabels {
    peer: PeerStatus,
    error: OutgoingConnectionErrorError,
}

#[derive(Encode, Hash, Clone, Eq, PartialEq, Copy)]
enum PeerStatus {
    Known,
    Unknown,
}

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
enum OutgoingConnectionErrorError {
    Banned,
    ConnectionLimit,
    LocalPeerId,
    NoAddresses,
    DialPeerConditionFalse,
    Aborted,
    InvalidPeerId,
    WrongPeerId,
    ConnectionIo,
    TransportMultiaddrNotSupported,
    TransportOther,
}

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
struct IncomingConnectionErrorLabels {
    error: PendingInboundConnectionError,
}

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
enum PendingInboundConnectionError {
    WrongPeerId,
    TransportErrorMultiaddrNotSupported,
    TransportErrorOther,
    Aborted,
    Io,
    ConnectionLimit,
}

impl<TTransErr> From<&libp2p_swarm::PendingInboundConnectionError<TTransErr>>
    for PendingInboundConnectionError
{
    fn from(error: &libp2p_swarm::PendingInboundConnectionError<TTransErr>) -> Self {
        match error {
            libp2p_swarm::PendingInboundConnectionError::WrongPeerId { .. } => {
                PendingInboundConnectionError::WrongPeerId
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
            libp2p_swarm::PendingInboundConnectionError::IO(_) => PendingInboundConnectionError::Io,
        }
    }
}
