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

//! [`NetworkBehaviour`] to act as a circuit relay v2 **relay**.

pub(crate) mod handler;
pub(crate) mod rate_limiter;
use std::{
    collections::{hash_map, HashMap, HashSet, VecDeque},
    num::NonZeroU32,
    ops::Add,
    task::{Context, Poll},
    time::Duration,
};

use either::Either;
use libp2p_core::{multiaddr::Protocol, transport::PortUse, ConnectedPoint, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionClosed, FromSwarm},
    dummy, ConnectionDenied, ConnectionId, ExternalAddresses, NetworkBehaviour, NotifyHandler,
    THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use web_time::Instant;

use crate::{
    behaviour::handler::Handler,
    multiaddr_ext::MultiaddrExt,
    proto,
    protocol::{inbound_hop, outbound_stop},
};

/// Configuration for the relay [`Behaviour`].
///
/// # Panics
///
/// [`Config::max_circuit_duration`] may not exceed [`u32::MAX`].
pub struct Config {
    pub max_reservations: usize,
    pub max_reservations_per_peer: usize,
    pub reservation_duration: Duration,
    pub reservation_rate_limiters: Vec<Box<dyn rate_limiter::RateLimiter>>,

    pub max_circuits: usize,
    pub max_circuits_per_peer: usize,
    pub max_circuit_duration: Duration,
    pub max_circuit_bytes: u64,
    pub circuit_src_rate_limiters: Vec<Box<dyn rate_limiter::RateLimiter>>,
}

impl Config {
    pub fn reservation_rate_per_peer(mut self, limit: NonZeroU32, interval: Duration) -> Self {
        self.reservation_rate_limiters
            .push(rate_limiter::new_per_peer(
                rate_limiter::GenericRateLimiterConfig { limit, interval },
            ));
        self
    }

    pub fn circuit_src_per_peer(mut self, limit: NonZeroU32, interval: Duration) -> Self {
        self.circuit_src_rate_limiters
            .push(rate_limiter::new_per_peer(
                rate_limiter::GenericRateLimiterConfig { limit, interval },
            ));
        self
    }

    pub fn reservation_rate_per_ip(mut self, limit: NonZeroU32, interval: Duration) -> Self {
        self.reservation_rate_limiters
            .push(rate_limiter::new_per_ip(
                rate_limiter::GenericRateLimiterConfig { limit, interval },
            ));
        self
    }

    pub fn circuit_src_per_ip(mut self, limit: NonZeroU32, interval: Duration) -> Self {
        self.circuit_src_rate_limiters
            .push(rate_limiter::new_per_ip(
                rate_limiter::GenericRateLimiterConfig { limit, interval },
            ));
        self
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("max_reservations", &self.max_reservations)
            .field("max_reservations_per_peer", &self.max_reservations_per_peer)
            .field("reservation_duration", &self.reservation_duration)
            .field(
                "reservation_rate_limiters",
                &format!("[{} rate limiters]", self.reservation_rate_limiters.len()),
            )
            .field("max_circuits", &self.max_circuits)
            .field("max_circuits_per_peer", &self.max_circuits_per_peer)
            .field("max_circuit_duration", &self.max_circuit_duration)
            .field("max_circuit_bytes", &self.max_circuit_bytes)
            .field(
                "circuit_src_rate_limiters",
                &format!("[{} rate limiters]", self.circuit_src_rate_limiters.len()),
            )
            .finish()
    }
}

impl Default for Config {
    fn default() -> Self {
        let reservation_rate_limiters = vec![
            // For each peer ID one reservation every 2 minutes with up
            // to 30 reservations per hour.
            rate_limiter::new_per_peer(rate_limiter::GenericRateLimiterConfig {
                limit: NonZeroU32::new(30).expect("30 > 0"),
                interval: Duration::from_secs(60 * 2),
            }),
            // For each IP address one reservation every minute with up
            // to 60 reservations per hour.
            rate_limiter::new_per_ip(rate_limiter::GenericRateLimiterConfig {
                limit: NonZeroU32::new(60).expect("60 > 0"),
                interval: Duration::from_secs(60),
            }),
        ];

        let circuit_src_rate_limiters = vec![
            // For each source peer ID one circuit every 2 minute with up to 30 circuits per hour.
            rate_limiter::new_per_peer(rate_limiter::GenericRateLimiterConfig {
                limit: NonZeroU32::new(30).expect("30 > 0"),
                interval: Duration::from_secs(60 * 2),
            }),
            // For each source IP address one circuit every minute with up to 60 circuits per hour.
            rate_limiter::new_per_ip(rate_limiter::GenericRateLimiterConfig {
                limit: NonZeroU32::new(60).expect("60 > 0"),
                interval: Duration::from_secs(60),
            }),
        ];

        Config {
            max_reservations: 128,
            max_reservations_per_peer: 4,
            reservation_duration: Duration::from_secs(60 * 60),
            reservation_rate_limiters,

            max_circuits: 16,
            max_circuits_per_peer: 4,
            max_circuit_duration: Duration::from_secs(2 * 60),
            max_circuit_bytes: 1 << 17, // 128 kibibyte
            circuit_src_rate_limiters,
        }
    }
}

/// The events produced by the relay `Behaviour`.
#[derive(Debug)]
pub enum Event {
    /// An inbound reservation request has been accepted.
    ReservationReqAccepted {
        src_peer_id: PeerId,
        /// Indicates whether the request replaces an existing reservation.
        renewed: bool,
    },
    /// Accepting an inbound reservation request failed.
    #[deprecated(
        note = "Will be removed in favor of logging them internally, see <https://github.com/libp2p/rust-libp2p/issues/4757> for details."
    )]
    ReservationReqAcceptFailed {
        src_peer_id: PeerId,
        error: inbound_hop::Error,
    },
    /// An inbound reservation request has been denied.
    ReservationReqDenied { src_peer_id: PeerId },
    /// Denying an inbound reservation request has failed.
    #[deprecated(
        note = "Will be removed in favor of logging them internally, see <https://github.com/libp2p/rust-libp2p/issues/4757> for details."
    )]
    ReservationReqDenyFailed {
        src_peer_id: PeerId,
        error: inbound_hop::Error,
    },
    /// An inbound reservation has timed out.
    ReservationTimedOut { src_peer_id: PeerId },
    /// An inbound circuit request has been denied.
    CircuitReqDenied {
        src_peer_id: PeerId,
        dst_peer_id: PeerId,
    },
    /// Denying an inbound circuit request failed.
    #[deprecated(
        note = "Will be removed in favor of logging them internally, see <https://github.com/libp2p/rust-libp2p/issues/4757> for details."
    )]
    CircuitReqDenyFailed {
        src_peer_id: PeerId,
        dst_peer_id: PeerId,
        error: inbound_hop::Error,
    },
    /// An inbound circuit request has been accepted.
    CircuitReqAccepted {
        src_peer_id: PeerId,
        dst_peer_id: PeerId,
    },
    /// An outbound connect for an inbound circuit request failed.
    #[deprecated(
        note = "Will be removed in favor of logging them internally, see <https://github.com/libp2p/rust-libp2p/issues/4757> for details."
    )]
    CircuitReqOutboundConnectFailed {
        src_peer_id: PeerId,
        dst_peer_id: PeerId,
        error: outbound_stop::Error,
    },
    /// Accepting an inbound circuit request failed.
    #[deprecated(
        note = "Will be removed in favor of logging them internally, see <https://github.com/libp2p/rust-libp2p/issues/4757> for details."
    )]
    CircuitReqAcceptFailed {
        src_peer_id: PeerId,
        dst_peer_id: PeerId,
        error: inbound_hop::Error,
    },
    /// An inbound circuit has closed.
    CircuitClosed {
        src_peer_id: PeerId,
        dst_peer_id: PeerId,
        error: Option<std::io::Error>,
    },
}

/// [`NetworkBehaviour`] implementation of the relay server
/// functionality of the circuit relay v2 protocol.
pub struct Behaviour {
    config: Config,

    local_peer_id: PeerId,

    reservations: HashMap<PeerId, HashSet<ConnectionId>>,
    circuits: CircuitsTracker,

    /// Queue of actions to return when polled.
    queued_actions: VecDeque<ToSwarm<Event, THandlerInEvent<Self>>>,

    external_addresses: ExternalAddresses,
}

impl Behaviour {
    pub fn new(local_peer_id: PeerId, config: Config) -> Self {
        Self {
            config,
            local_peer_id,
            reservations: Default::default(),
            circuits: Default::default(),
            queued_actions: Default::default(),
            external_addresses: Default::default(),
        }
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            connection_id,
            ..
        }: ConnectionClosed,
    ) {
        if let hash_map::Entry::Occupied(mut peer) = self.reservations.entry(peer_id) {
            peer.get_mut().remove(&connection_id);
            if peer.get().is_empty() {
                peer.remove();
            }
        }

        for circuit in self
            .circuits
            .remove_by_connection(peer_id, connection_id)
            .iter()
            // Only emit [`CircuitClosed`] for accepted requests.
            .filter(|c| matches!(c.status, CircuitStatus::Accepted))
        {
            self.queued_actions
                .push_back(ToSwarm::GenerateEvent(Event::CircuitClosed {
                    src_peer_id: circuit.src_peer_id,
                    dst_peer_id: circuit.dst_peer_id,
                    error: Some(std::io::ErrorKind::ConnectionAborted.into()),
                }));
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Either<Handler, dummy::ConnectionHandler>;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if local_addr.is_relayed() {
            // Deny all substreams on relayed connection.
            return Ok(Either::Right(dummy::ConnectionHandler));
        }

        Ok(Either::Left(Handler::new(
            handler::Config {
                reservation_duration: self.config.reservation_duration,
                max_circuit_duration: self.config.max_circuit_duration,
                max_circuit_bytes: self.config.max_circuit_bytes,
            },
            ConnectedPoint::Listener {
                local_addr: local_addr.clone(),
                send_back_addr: remote_addr.clone(),
            },
        )))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if addr.is_relayed() {
            // Deny all substreams on relayed connection.
            return Ok(Either::Right(dummy::ConnectionHandler));
        }

        Ok(Either::Left(Handler::new(
            handler::Config {
                reservation_duration: self.config.reservation_duration,
                max_circuit_duration: self.config.max_circuit_duration,
                max_circuit_bytes: self.config.max_circuit_bytes,
            },
            ConnectedPoint::Dialer {
                address: addr.clone(),
                role_override,
                port_use,
            },
        )))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.external_addresses.on_swarm_event(&event);

        if let FromSwarm::ConnectionClosed(connection_closed) = event {
            self.on_connection_closed(connection_closed)
        }
    }

    fn on_connection_handler_event(
        &mut self,
        event_source: PeerId,
        connection: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        let event = match event {
            Either::Left(e) => e,
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            Either::Right(v) => libp2p_core::util::unreachable(v),
        };

        match event {
            handler::Event::ReservationReqReceived {
                inbound_reservation_req,
                endpoint,
                renewed,
            } => {
                let now = Instant::now();

                assert!(
                    !endpoint.is_relayed(),
                    "`dummy::ConnectionHandler` handles relayed connections. It \
                     denies all inbound substreams."
                );

                let action = if
                // Deny if it is a new reservation and exceeds
                // `max_reservations_per_peer`.
                (!renewed
                    && self
                        .reservations
                        .get(&event_source)
                        .map(|cs| cs.len())
                        .unwrap_or(0)
                        > self.config.max_reservations_per_peer)
                    // Deny if it exceeds `max_reservations`.
                    || self
                        .reservations
                        .values()
                        .map(|cs| cs.len())
                        .sum::<usize>()
                        >= self.config.max_reservations
                    // Deny if it exceeds the allowed rate of reservations.
                    || !self
                        .config
                        .reservation_rate_limiters
                        .iter_mut()
                        .all(|limiter| {
                            limiter.try_next(event_source, endpoint.get_remote_address(), now)
                        }) {
                    ToSwarm::NotifyHandler {
                        handler: NotifyHandler::One(connection),
                        peer_id: event_source,
                        event: Either::Left(handler::In::DenyReservationReq {
                            inbound_reservation_req,
                            status: proto::Status::RESOURCE_LIMIT_EXCEEDED,
                        }),
                    }
                } else {
                    // Accept reservation.
                    self.reservations
                        .entry(event_source)
                        .or_default()
                        .insert(connection);

                    ToSwarm::NotifyHandler {
                        handler: NotifyHandler::One(connection),
                        peer_id: event_source,
                        event: Either::Left(handler::In::AcceptReservationReq {
                            inbound_reservation_req,
                            addrs: self
                                .external_addresses
                                .iter()
                                .cloned()
                                // Add local peer ID in case it isn't present yet.
                                .filter_map(|a| match a.iter().last()? {
                                    Protocol::P2p(_) => Some(a),
                                    _ => Some(a.with(Protocol::P2p(self.local_peer_id))),
                                })
                                .collect(),
                        }),
                    }
                };

                self.queued_actions.push_back(action);
            }
            handler::Event::ReservationReqAccepted { renewed } => {
                // Ensure local eventual consistent reservation state matches handler (source of
                // truth).
                self.reservations
                    .entry(event_source)
                    .or_default()
                    .insert(connection);

                self.queued_actions.push_back(ToSwarm::GenerateEvent(
                    Event::ReservationReqAccepted {
                        src_peer_id: event_source,
                        renewed,
                    },
                ));
            }
            handler::Event::ReservationReqAcceptFailed { error } => {
                #[allow(deprecated)]
                self.queued_actions.push_back(ToSwarm::GenerateEvent(
                    Event::ReservationReqAcceptFailed {
                        src_peer_id: event_source,
                        error,
                    },
                ));
            }
            handler::Event::ReservationReqDenied {} => {
                self.queued_actions.push_back(ToSwarm::GenerateEvent(
                    Event::ReservationReqDenied {
                        src_peer_id: event_source,
                    },
                ));
            }
            handler::Event::ReservationReqDenyFailed { error } => {
                #[allow(deprecated)]
                self.queued_actions.push_back(ToSwarm::GenerateEvent(
                    Event::ReservationReqDenyFailed {
                        src_peer_id: event_source,
                        error,
                    },
                ));
            }
            handler::Event::ReservationTimedOut {} => {
                match self.reservations.entry(event_source) {
                    hash_map::Entry::Occupied(mut peer) => {
                        peer.get_mut().remove(&connection);
                        if peer.get().is_empty() {
                            peer.remove();
                        }
                    }
                    hash_map::Entry::Vacant(_) => {
                        unreachable!(
                            "Expect to track timed out reservation with peer {:?} on connection {:?}",
                            event_source,
                            connection,
                        );
                    }
                }

                self.queued_actions
                    .push_back(ToSwarm::GenerateEvent(Event::ReservationTimedOut {
                        src_peer_id: event_source,
                    }));
            }
            handler::Event::CircuitReqReceived {
                inbound_circuit_req,
                endpoint,
            } => {
                let now = Instant::now();

                assert!(
                    !endpoint.is_relayed(),
                    "`dummy::ConnectionHandler` handles relayed connections. It \
                     denies all inbound substreams."
                );

                let action = if self.circuits.num_circuits_of_peer(event_source)
                    > self.config.max_circuits_per_peer
                    || self.circuits.len() >= self.config.max_circuits
                    || !self
                        .config
                        .circuit_src_rate_limiters
                        .iter_mut()
                        .all(|limiter| {
                            limiter.try_next(event_source, endpoint.get_remote_address(), now)
                        }) {
                    // Deny circuit exceeding limits.
                    ToSwarm::NotifyHandler {
                        handler: NotifyHandler::One(connection),
                        peer_id: event_source,
                        event: Either::Left(handler::In::DenyCircuitReq {
                            circuit_id: None,
                            inbound_circuit_req,
                            status: proto::Status::RESOURCE_LIMIT_EXCEEDED,
                        }),
                    }
                } else if let Some(dst_conn) = self
                    .reservations
                    .get(&inbound_circuit_req.dst())
                    .and_then(|cs| cs.iter().next())
                {
                    // Accept circuit request if reservation present.
                    let circuit_id = self.circuits.insert(Circuit {
                        status: CircuitStatus::Accepting,
                        src_peer_id: event_source,
                        src_connection_id: connection,
                        dst_peer_id: inbound_circuit_req.dst(),
                        dst_connection_id: *dst_conn,
                    });

                    ToSwarm::NotifyHandler {
                        handler: NotifyHandler::One(*dst_conn),
                        peer_id: event_source,
                        event: Either::Left(handler::In::NegotiateOutboundConnect {
                            circuit_id,
                            inbound_circuit_req,
                            src_peer_id: event_source,
                            src_connection_id: connection,
                        }),
                    }
                } else {
                    // Deny circuit request if no reservation present.
                    ToSwarm::NotifyHandler {
                        handler: NotifyHandler::One(connection),
                        peer_id: event_source,
                        event: Either::Left(handler::In::DenyCircuitReq {
                            circuit_id: None,
                            inbound_circuit_req,
                            status: proto::Status::NO_RESERVATION,
                        }),
                    }
                };
                self.queued_actions.push_back(action);
            }
            handler::Event::CircuitReqDenied {
                circuit_id,
                dst_peer_id,
            } => {
                if let Some(circuit_id) = circuit_id {
                    self.circuits.remove(circuit_id);
                }

                self.queued_actions
                    .push_back(ToSwarm::GenerateEvent(Event::CircuitReqDenied {
                        src_peer_id: event_source,
                        dst_peer_id,
                    }));
            }
            handler::Event::CircuitReqDenyFailed {
                circuit_id,
                dst_peer_id,
                error,
            } => {
                if let Some(circuit_id) = circuit_id {
                    self.circuits.remove(circuit_id);
                }

                #[allow(deprecated)]
                self.queued_actions.push_back(ToSwarm::GenerateEvent(
                    Event::CircuitReqDenyFailed {
                        src_peer_id: event_source,
                        dst_peer_id,
                        error,
                    },
                ));
            }
            handler::Event::OutboundConnectNegotiated {
                circuit_id,
                src_peer_id,
                src_connection_id,
                inbound_circuit_req,
                dst_stream,
                dst_pending_data,
            } => {
                self.queued_actions.push_back(ToSwarm::NotifyHandler {
                    handler: NotifyHandler::One(src_connection_id),
                    peer_id: src_peer_id,
                    event: Either::Left(handler::In::AcceptAndDriveCircuit {
                        circuit_id,
                        dst_peer_id: event_source,
                        inbound_circuit_req,
                        dst_stream,
                        dst_pending_data,
                    }),
                });
            }
            handler::Event::OutboundConnectNegotiationFailed {
                circuit_id,
                src_peer_id,
                src_connection_id,
                inbound_circuit_req,
                status,
                error,
            } => {
                self.queued_actions.push_back(ToSwarm::NotifyHandler {
                    handler: NotifyHandler::One(src_connection_id),
                    peer_id: src_peer_id,
                    event: Either::Left(handler::In::DenyCircuitReq {
                        circuit_id: Some(circuit_id),
                        inbound_circuit_req,
                        status,
                    }),
                });
                #[allow(deprecated)]
                self.queued_actions.push_back(ToSwarm::GenerateEvent(
                    Event::CircuitReqOutboundConnectFailed {
                        src_peer_id,
                        dst_peer_id: event_source,
                        error,
                    },
                ));
            }
            handler::Event::CircuitReqAccepted {
                dst_peer_id,
                circuit_id,
            } => {
                self.circuits.accepted(circuit_id);
                self.queued_actions
                    .push_back(ToSwarm::GenerateEvent(Event::CircuitReqAccepted {
                        src_peer_id: event_source,
                        dst_peer_id,
                    }));
            }
            handler::Event::CircuitReqAcceptFailed {
                dst_peer_id,
                circuit_id,
                error,
            } => {
                self.circuits.remove(circuit_id);
                #[allow(deprecated)]
                self.queued_actions.push_back(ToSwarm::GenerateEvent(
                    Event::CircuitReqAcceptFailed {
                        src_peer_id: event_source,
                        dst_peer_id,
                        error,
                    },
                ));
            }
            handler::Event::CircuitClosed {
                dst_peer_id,
                circuit_id,
                error,
            } => {
                self.circuits.remove(circuit_id);

                self.queued_actions
                    .push_back(ToSwarm::GenerateEvent(Event::CircuitClosed {
                        src_peer_id: event_source,
                        dst_peer_id,
                        error,
                    }));
            }
        }
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self))]
    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(to_swarm) = self.queued_actions.pop_front() {
            return Poll::Ready(to_swarm);
        }

        Poll::Pending
    }
}

#[derive(Default)]
struct CircuitsTracker {
    next_id: CircuitId,
    circuits: HashMap<CircuitId, Circuit>,
}

impl CircuitsTracker {
    fn len(&self) -> usize {
        self.circuits.len()
    }

    fn insert(&mut self, circuit: Circuit) -> CircuitId {
        let id = self.next_id;
        self.next_id = self.next_id + 1;

        self.circuits.insert(id, circuit);

        id
    }

    fn accepted(&mut self, circuit_id: CircuitId) {
        if let Some(c) = self.circuits.get_mut(&circuit_id) {
            c.status = CircuitStatus::Accepted;
        };
    }

    fn remove(&mut self, circuit_id: CircuitId) -> Option<Circuit> {
        self.circuits.remove(&circuit_id)
    }

    fn remove_by_connection(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
    ) -> Vec<Circuit> {
        let mut removed = vec![];

        self.circuits.retain(|_circuit_id, circuit| {
            let is_src =
                circuit.src_peer_id == peer_id && circuit.src_connection_id == connection_id;
            let is_dst =
                circuit.dst_peer_id == peer_id && circuit.dst_connection_id == connection_id;

            if is_src || is_dst {
                removed.push(circuit.clone());
                // Remove circuit from HashMap.
                false
            } else {
                // Retain circuit in HashMap.
                true
            }
        });

        removed
    }

    fn num_circuits_of_peer(&self, peer: PeerId) -> usize {
        self.circuits
            .iter()
            .filter(|(_, c)| c.src_peer_id == peer || c.dst_peer_id == peer)
            .count()
    }
}

#[derive(Clone)]
struct Circuit {
    src_peer_id: PeerId,
    src_connection_id: ConnectionId,
    dst_peer_id: PeerId,
    dst_connection_id: ConnectionId,
    status: CircuitStatus,
}

#[derive(Clone)]
enum CircuitStatus {
    Accepting,
    Accepted,
}

#[derive(Default, Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct CircuitId(u64);

impl Add<u64> for CircuitId {
    type Output = CircuitId;

    fn add(self, rhs: u64) -> Self {
        CircuitId(self.0 + rhs)
    }
}
