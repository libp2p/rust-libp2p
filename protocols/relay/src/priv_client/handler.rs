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

use crate::priv_client::transport;
use crate::protocol::{self, inbound_stop, outbound_hop};
use crate::{proto, HOP_PROTOCOL_NAME, STOP_PROTOCOL_NAME};
use either::Either;
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, FutureExt};
use futures::sink::SinkExt;
use futures::stream::{FuturesUnordered, StreamExt};
use futures_timer::Delay;
use instant::Instant;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::upgrade::ReadyUpgrade;
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, KeepAlive, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol,
};
use log::debug;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::task::{Context, Poll};
use std::time::Duration;

/// The maximum number of circuits being denied concurrently.
///
/// Circuits to be denied exceeding the limit are dropped.
const MAX_NUMBER_DENYING_CIRCUIT: usize = 8;

pub enum In {
    Reserve {
        to_listener: mpsc::Sender<transport::ToListenerMsg>,
    },
    EstablishCircuit {
        dst_peer_id: PeerId,
        send_back: oneshot::Sender<Result<super::Connection, ()>>,
    },
}

impl fmt::Debug for In {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            In::Reserve { to_listener: _ } => f.debug_struct("In::Reserve").finish(),
            In::EstablishCircuit {
                dst_peer_id,
                send_back: _,
            } => f
                .debug_struct("In::EstablishCircuit")
                .field("dst_peer_id", dst_peer_id)
                .finish(),
        }
    }
}

#[derive(Debug)]
pub enum Event {
    ReservationReqAccepted {
        /// Indicates whether the request replaces an existing reservation.
        renewal: bool,
        limit: Option<protocol::Limit>,
    },
    ReservationReqFailed {
        /// Indicates whether the request replaces an existing reservation.
        renewal: bool,
        error: StreamUpgradeError<outbound_hop::ReservationFailedReason>,
    },
    /// An outbound circuit has been established.
    OutboundCircuitEstablished { limit: Option<protocol::Limit> },
    OutboundCircuitReqFailed {
        error: StreamUpgradeError<outbound_hop::CircuitFailedReason>,
    },
    /// An inbound circuit has been established.
    InboundCircuitEstablished {
        src_peer_id: PeerId,
        limit: Option<protocol::Limit>,
    },
    /// An inbound circuit request has been denied.
    InboundCircuitReqDenied { src_peer_id: PeerId },
    /// Denying an inbound circuit request failed.
    InboundCircuitReqDenyFailed {
        src_peer_id: PeerId,
        error: inbound_stop::UpgradeError,
    },
}

pub struct Handler {
    local_peer_id: PeerId,
    remote_peer_id: PeerId,
    remote_addr: Multiaddr,
    /// A pending fatal error that results in the connection being closed.
    pending_error: Option<
        StreamUpgradeError<
            Either<inbound_stop::FatalUpgradeError, outbound_hop::FatalUpgradeError>,
        >,
    >,
    /// Until when to keep the connection alive.
    keep_alive: KeepAlive,

    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
            <Self as ConnectionHandler>::Error,
        >,
    >,

    wait_for_reserve_outbound_stream: VecDeque<mpsc::Sender<transport::ToListenerMsg>>,
    reserve_futs: FuturesUnordered<
        BoxFuture<'static, Result<outbound_hop::Output, outbound_hop::UpgradeError>>,
    >,

    wait_for_connection_outbound_stream: VecDeque<outbound_hop::Command>,
    circuit_connection_futs: FuturesUnordered<
        BoxFuture<'static, Result<Option<outbound_hop::Output>, outbound_hop::UpgradeError>>,
    >,

    reservation: Reservation,

    /// Tracks substreams lent out to the transport.
    ///
    /// Contains a [`futures::future::Future`] for each lend out substream that
    /// resolves once the substream is dropped.
    ///
    /// Once all substreams are dropped and this handler has no other work,
    /// [`KeepAlive::Until`] can be set, allowing the connection to be closed
    /// eventually.
    alive_lend_out_substreams: FuturesUnordered<oneshot::Receiver<void::Void>>,

    open_circuit_futs: FuturesUnordered<
        BoxFuture<'static, Result<inbound_stop::Circuit, inbound_stop::FatalUpgradeError>>,
    >,

    circuit_deny_futs: HashMap<PeerId, BoxFuture<'static, Result<(), inbound_stop::UpgradeError>>>,

    /// Futures that try to send errors to the transport.
    ///
    /// We may drop errors if this handler ends up in a terminal state (by returning
    /// [`ConnectionHandlerEvent::Close`]).
    send_error_futs: FuturesUnordered<BoxFuture<'static, ()>>,
}

impl Handler {
    pub fn new(local_peer_id: PeerId, remote_peer_id: PeerId, remote_addr: Multiaddr) -> Self {
        Self {
            local_peer_id,
            remote_peer_id,
            remote_addr,
            queued_events: Default::default(),
            pending_error: Default::default(),
            wait_for_reserve_outbound_stream: Default::default(),
            reserve_futs: Default::default(),
            wait_for_connection_outbound_stream: Default::default(),
            circuit_connection_futs: Default::default(),
            reservation: Reservation::None,
            alive_lend_out_substreams: Default::default(),
            open_circuit_futs: Default::default(),
            circuit_deny_futs: Default::default(),
            send_error_futs: Default::default(),
            keep_alive: KeepAlive::Yes,
        }
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, .. }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        // Try to process the error for a reservation
        if let Some(mut to_listener) = self.wait_for_reserve_outbound_stream.pop_front() {
            let non_fatal_error = match error {
                StreamUpgradeError::Timeout => StreamUpgradeError::Timeout,
                StreamUpgradeError::NegotiationFailed => StreamUpgradeError::NegotiationFailed,
                StreamUpgradeError::Io(e) => {
                    self.pending_error = Some(StreamUpgradeError::Io(e));
                    return;
                }
                StreamUpgradeError::Apply(_) => unreachable!("should not update"),
            };

            if self.pending_error.is_none() {
                self.send_error_futs.push(
                    async move {
                        let _ = to_listener
                            .send(transport::ToListenerMsg::Reservation(Err(())))
                            .await;
                    }
                    .boxed(),
                );
            } else {
                // Fatal error occurred, thus handler is closing as quickly as possible.
                // Transport is notified through dropping `to_listener`.
            }

            let renewal = self.reservation.failed();

            self.queued_events
                .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::ReservationReqFailed {
                        renewal,
                        error: non_fatal_error,
                    },
                ));

            return;
        }
        // Try to process the error for a connection
        let cmd = self.wait_for_connection_outbound_stream.pop_front().expect(
            "got a stream error without a pending connection command or a reserve listener",
        );

        let non_fatal_error = match error {
            StreamUpgradeError::Timeout => StreamUpgradeError::Timeout,
            StreamUpgradeError::NegotiationFailed => StreamUpgradeError::NegotiationFailed,
            StreamUpgradeError::Io(e) => {
                self.pending_error = Some(StreamUpgradeError::Io(e));
                return;
            }
            StreamUpgradeError::Apply(_) => unreachable!("should not update"),
        };

        let _ = cmd.send_back.send(Err(()));

        self.queued_events
            .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                Event::OutboundCircuitReqFailed {
                    error: non_fatal_error,
                },
            ));
    }

    fn insert_to_deny_futs(&mut self, circuit: inbound_stop::Circuit) {
        let src_peer_id = circuit.src_peer_id();

        if self.circuit_deny_futs.len() == MAX_NUMBER_DENYING_CIRCUIT
            && !self.circuit_deny_futs.contains_key(&src_peer_id)
        {
            log::warn!(
                "Dropping inbound circuit request to be denied from {:?} due to exceeding limit.",
                src_peer_id
            );
            return;
        }

        if self
            .circuit_deny_futs
            .insert(
                src_peer_id,
                circuit.deny(proto::Status::NO_RESERVATION).boxed(),
            )
            .is_some()
        {
            log::warn!("Dropping existing inbound circuit request to be denied from {:?} in favor of new one.", src_peer_id);
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = In;
    type ToBehaviour = Event;
    type Error = StreamUpgradeError<
        Either<inbound_stop::FatalUpgradeError, outbound_hop::FatalUpgradeError>,
    >;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(STOP_PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            In::Reserve { to_listener } => {
                self.wait_for_reserve_outbound_stream.push_back(to_listener);
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(ReadyUpgrade::new(HOP_PROTOCOL_NAME), ()),
                    });
            }
            In::EstablishCircuit {
                send_back,
                dst_peer_id,
            } => {
                self.wait_for_connection_outbound_stream
                    .push_back(outbound_hop::Command::new(dst_peer_id, send_back));
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(ReadyUpgrade::new(HOP_PROTOCOL_NAME), ()),
                    });
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
            Self::Error,
        >,
    > {
        // Check for a pending (fatal) error.
        if let Some(err) = self.pending_error.take() {
            // The handler will not be polled again by the `Swarm`.
            return Poll::Ready(ConnectionHandlerEvent::Close(err));
        }

        // Reservations
        if let Poll::Ready(Some(result)) = self.reserve_futs.poll_next_unpin(cx) {
            let event = match result {
                Ok(outbound_hop::Output::Reservation {
                    renewal_timeout,
                    addrs,
                    limit,
                    to_listener,
                }) => ConnectionHandlerEvent::NotifyBehaviour(self.reservation.accepted(
                    renewal_timeout,
                    addrs,
                    to_listener,
                    self.local_peer_id,
                    limit,
                )),
                Err(err) => match err {
                    outbound_hop::UpgradeError::ReservationFailed(e) => {
                        let renewal = self.reservation.failed();
                        return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                            Event::ReservationReqFailed {
                                renewal,
                                error: StreamUpgradeError::Apply(e),
                            },
                        ));
                    }
                    outbound_hop::UpgradeError::Fatal(e) => {
                        ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(Either::Right(e)))
                    }
                    outbound_hop::UpgradeError::CircuitFailed(_) => {
                        unreachable!("do not emit `CircuitFailed` for reservation")
                    }
                },
                _ => unreachable!("do not emit 'Output::Circuit' for reservation"),
            };

            return Poll::Ready(event);
        }

        // Circuit connections
        if let Poll::Ready(Some(res)) = self.circuit_connection_futs.poll_next_unpin(cx) {
            let opt = match res {
                Ok(Some(outbound_hop::Output::Circuit { limit })) => {
                    Some(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::OutboundCircuitEstablished { limit },
                    ))
                }
                Ok(None) => None,
                Err(err) => {
                    let res = match err {
                        outbound_hop::UpgradeError::CircuitFailed(e) => {
                            ConnectionHandlerEvent::NotifyBehaviour(
                                Event::OutboundCircuitReqFailed {
                                    error: StreamUpgradeError::Apply(e),
                                },
                            )
                        }
                        outbound_hop::UpgradeError::Fatal(e) => ConnectionHandlerEvent::Close(
                            StreamUpgradeError::Apply(Either::Right(e)),
                        ),
                        outbound_hop::UpgradeError::ReservationFailed(_) => {
                            unreachable!("do not emit `ReservationFailed` for connection")
                        }
                    };

                    Some(res)
                }
                _ => unreachable!("do not emit 'Output::Reservation' for connection"),
            };

            if let Some(event) = opt {
                return Poll::Ready(event);
            }
        }

        // Return queued events.
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        if let Poll::Ready(Some(circuit_res)) = self.open_circuit_futs.poll_next_unpin(cx) {
            match circuit_res {
                Ok(circuit) => match &mut self.reservation {
                    Reservation::Accepted { pending_msgs, .. }
                    | Reservation::Renewing { pending_msgs, .. } => {
                        let src_peer_id = circuit.src_peer_id();
                        let limit = circuit.limit();

                        let (tx, rx) = oneshot::channel();
                        self.alive_lend_out_substreams.push(rx);
                        let connection = super::ConnectionState::new_inbound(circuit, tx);

                        pending_msgs.push_back(
                            transport::ToListenerMsg::IncomingRelayedConnection {
                                stream: super::Connection { state: connection },
                                src_peer_id,
                                relay_peer_id: self.remote_peer_id,
                                relay_addr: self.remote_addr.clone(),
                            },
                        );
                        return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                            Event::InboundCircuitEstablished { src_peer_id, limit },
                        ));
                    }
                    Reservation::None => {
                        self.insert_to_deny_futs(circuit);
                    }
                },
                Err(e) => {
                    return Poll::Ready(ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(
                        Either::Left(e),
                    )));
                }
            }
        }

        if let Poll::Ready(Some(to_listener)) = self.reservation.poll(cx) {
            self.wait_for_reserve_outbound_stream.push_back(to_listener);

            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(HOP_PROTOCOL_NAME), ()),
            });
        }

        // Deny incoming circuit requests.
        let maybe_event =
            self.circuit_deny_futs
                .iter_mut()
                .find_map(|(src_peer_id, fut)| match fut.poll_unpin(cx) {
                    Poll::Ready(Ok(())) => Some((
                        *src_peer_id,
                        Event::InboundCircuitReqDenied {
                            src_peer_id: *src_peer_id,
                        },
                    )),
                    Poll::Ready(Err(error)) => Some((
                        *src_peer_id,
                        Event::InboundCircuitReqDenyFailed {
                            src_peer_id: *src_peer_id,
                            error,
                        },
                    )),
                    Poll::Pending => None,
                });
        if let Some((src_peer_id, event)) = maybe_event {
            self.circuit_deny_futs.remove(&src_peer_id);
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        // Send errors to transport.
        while let Poll::Ready(Some(())) = self.send_error_futs.poll_next_unpin(cx) {}

        // Check status of lend out substreams.
        loop {
            match self.alive_lend_out_substreams.poll_next_unpin(cx) {
                Poll::Ready(Some(Err(oneshot::Canceled))) => {}
                Poll::Ready(Some(Ok(v))) => void::unreachable(v),
                Poll::Ready(None) | Poll::Pending => break,
            }
        }

        // Update keep-alive handling.
        if matches!(self.reservation, Reservation::None)
            && self.alive_lend_out_substreams.is_empty()
            && self.circuit_deny_futs.is_empty()
        {
            match self.keep_alive {
                KeepAlive::Yes => {
                    self.keep_alive = KeepAlive::Until(Instant::now() + Duration::from_secs(10));
                }
                KeepAlive::Until(_) => {}
                KeepAlive::No => panic!("Handler never sets KeepAlive::No."),
            }
        } else {
            self.keep_alive = KeepAlive::Yes;
        }

        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: stream,
                ..
            }) => {
                self.open_circuit_futs
                    .push(inbound_stop::open_circuit(stream).boxed());
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: stream,
                ..
            }) => {
                if let Some(to_listener) = self.wait_for_reserve_outbound_stream.pop_front() {
                    self.reserve_futs.push(
                        outbound_hop::send_reserve_message_and_process_response(
                            stream,
                            to_listener,
                        )
                        .boxed(),
                    );
                    return;
                }

                let con_command = self.wait_for_connection_outbound_stream.pop_front().expect(
                    "opened a stream without a pending connection command or a reserve listener",
                );

                let (tx, rx) = oneshot::channel();
                self.alive_lend_out_substreams.push(rx);

                self.circuit_connection_futs.push(
                    outbound_hop::send_connection_message_and_process_response(
                        stream,
                        self.remote_peer_id,
                        con_command,
                        tx,
                    )
                    .boxed(),
                )
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::ListenUpgradeError(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_) => {}
        }
    }
}

enum Reservation {
    /// The Reservation is accepted by the relay.
    Accepted {
        renewal_timeout: Delay,
        /// Buffer of messages to be send to the transport listener.
        pending_msgs: VecDeque<transport::ToListenerMsg>,
        to_listener: mpsc::Sender<transport::ToListenerMsg>,
    },
    /// The reservation is being renewed with the relay.
    Renewing {
        /// Buffer of messages to be send to the transport listener.
        pending_msgs: VecDeque<transport::ToListenerMsg>,
    },
    None,
}

impl Reservation {
    fn accepted(
        &mut self,
        renewal_timeout: Delay,
        addrs: Vec<Multiaddr>,
        to_listener: mpsc::Sender<transport::ToListenerMsg>,
        local_peer_id: PeerId,
        limit: Option<protocol::Limit>,
    ) -> Event {
        let (renewal, mut pending_msgs) = match std::mem::replace(self, Self::None) {
            Reservation::Accepted { pending_msgs, .. }
            | Reservation::Renewing { pending_msgs, .. } => (true, pending_msgs),
            Reservation::None => (false, VecDeque::new()),
        };

        pending_msgs.push_back(transport::ToListenerMsg::Reservation(Ok(
            transport::Reservation {
                addrs: addrs
                    .into_iter()
                    .map(|a| {
                        a.with(Protocol::P2pCircuit)
                            .with(Protocol::P2p(local_peer_id))
                    })
                    .collect(),
            },
        )));

        *self = Reservation::Accepted {
            renewal_timeout,
            pending_msgs,
            to_listener,
        };

        Event::ReservationReqAccepted { renewal, limit }
    }

    /// Marks the current reservation as failed.
    ///
    /// Returns whether the reservation request was a renewal.
    fn failed(&mut self) -> bool {
        let renewal = matches!(
            self,
            Reservation::Accepted { .. } | Reservation::Renewing { .. }
        );

        *self = Reservation::None;

        renewal
    }

    fn forward_messages_to_transport_listener(&mut self, cx: &mut Context<'_>) {
        if let Reservation::Accepted {
            pending_msgs,
            to_listener,
            ..
        } = self
        {
            if !pending_msgs.is_empty() {
                match to_listener.poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        if let Err(e) = to_listener
                            .start_send(pending_msgs.pop_front().expect("Called !is_empty()."))
                        {
                            debug!("Failed to sent pending message to listener: {:?}", e);
                            *self = Reservation::None;
                        }
                    }
                    Poll::Ready(Err(e)) => {
                        debug!("Channel to listener failed: {:?}", e);
                        *self = Reservation::None;
                    }
                    Poll::Pending => {}
                }
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<mpsc::Sender<transport::ToListenerMsg>>> {
        self.forward_messages_to_transport_listener(cx);

        // Check renewal timeout if any.
        let (next_reservation, poll_val) = match std::mem::replace(self, Reservation::None) {
            Reservation::Accepted {
                mut renewal_timeout,
                pending_msgs,
                to_listener,
            } => match renewal_timeout.poll_unpin(cx) {
                Poll::Ready(()) => (
                    Reservation::Renewing { pending_msgs },
                    Poll::Ready(Some(to_listener)),
                ),
                Poll::Pending => (
                    Reservation::Accepted {
                        renewal_timeout,
                        pending_msgs,
                        to_listener,
                    },
                    Poll::Pending,
                ),
            },
            r => (r, Poll::Pending),
        };
        *self = next_reservation;

        poll_val
    }
}
