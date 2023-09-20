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
use crate::proto;
use crate::protocol::{self, inbound_stop, outbound_hop};
use either::Either;
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, FutureExt};
use futures::sink::SinkExt;
use futures::stream::{FuturesUnordered, StreamExt};
use futures_timer::Delay;
use instant::Instant;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
    ListenUpgradeError,
};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, KeepAlive, StreamUpgradeError, SubstreamProtocol,
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

    circuit_deny_futs:
        HashMap<PeerId, BoxFuture<'static, Result<(), protocol::inbound_stop::UpgradeError>>>,

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
            reservation: Reservation::None,
            alive_lend_out_substreams: Default::default(),
            circuit_deny_futs: Default::default(),
            send_error_futs: Default::default(),
            keep_alive: KeepAlive::Yes,
        }
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: inbound_circuit,
            ..
        }: FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
        match &mut self.reservation {
            Reservation::Accepted { pending_msgs, .. }
            | Reservation::Renewing { pending_msgs, .. } => {
                let src_peer_id = inbound_circuit.src_peer_id();
                let limit = inbound_circuit.limit();

                let (tx, rx) = oneshot::channel();
                self.alive_lend_out_substreams.push(rx);
                let connection = super::ConnectionState::new_inbound(inbound_circuit, tx);

                pending_msgs.push_back(transport::ToListenerMsg::IncomingRelayedConnection {
                    // stream: connection,
                    stream: super::Connection { state: connection },
                    src_peer_id,
                    relay_peer_id: self.remote_peer_id,
                    relay_addr: self.remote_addr.clone(),
                });

                self.queued_events
                    .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::InboundCircuitEstablished { src_peer_id, limit },
                    ));
            }
            Reservation::None => {
                let src_peer_id = inbound_circuit.src_peer_id();

                if self.circuit_deny_futs.len() == MAX_NUMBER_DENYING_CIRCUIT
                    && !self.circuit_deny_futs.contains_key(&src_peer_id)
                {
                    log::warn!(
                        "Dropping inbound circuit request to be denied from {:?} due to exceeding limit.",
                        src_peer_id,
                    );
                } else if self
                    .circuit_deny_futs
                    .insert(
                        src_peer_id,
                        inbound_circuit.deny(proto::Status::NO_RESERVATION).boxed(),
                    )
                    .is_some()
                {
                    log::warn!(
                            "Dropping existing inbound circuit request to be denied from {:?} in favor of new one.",
                            src_peer_id
                        )
                }
            }
        }
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: output,
            info,
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        match (output, info) {
            // Outbound reservation
            (
                outbound_hop::Output::Reservation {
                    renewal_timeout,
                    addrs,
                    limit,
                },
                OutboundOpenInfo::Reserve { to_listener },
            ) => {
                let event = self.reservation.accepted(
                    renewal_timeout,
                    addrs,
                    to_listener,
                    self.local_peer_id,
                    limit,
                );

                self.queued_events
                    .push_back(ConnectionHandlerEvent::NotifyBehaviour(event));
            }

            // Outbound circuit
            (
                outbound_hop::Output::Circuit {
                    substream,
                    read_buffer,
                    limit,
                },
                OutboundOpenInfo::Connect { send_back },
            ) => {
                let (tx, rx) = oneshot::channel();
                match send_back.send(Ok(super::Connection {
                    state: super::ConnectionState::new_outbound(substream, read_buffer, tx),
                })) {
                    Ok(()) => {
                        self.alive_lend_out_substreams.push(rx);
                        self.queued_events
                            .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                                Event::OutboundCircuitEstablished { limit },
                            ));
                    }
                    Err(_) => debug!(
                        "Oneshot to `client::transport::Dial` future dropped. \
                         Dropping established relayed connection to {:?}.",
                        self.remote_peer_id,
                    ),
                }
            }

            _ => unreachable!(),
        }
    }

    fn on_listen_upgrade_error(
        &mut self,
        ListenUpgradeError {
            error: inbound_stop::UpgradeError::Fatal(error),
            ..
        }: ListenUpgradeError<
            <Self as ConnectionHandler>::InboundOpenInfo,
            <Self as ConnectionHandler>::InboundProtocol,
        >,
    ) {
        self.pending_error = Some(StreamUpgradeError::Apply(Either::Left(error)));
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError {
            info: open_info,
            error,
        }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        match open_info {
            OutboundOpenInfo::Reserve { mut to_listener } => {
                let non_fatal_error = match error {
                    StreamUpgradeError::Timeout => StreamUpgradeError::Timeout,
                    StreamUpgradeError::NegotiationFailed => StreamUpgradeError::NegotiationFailed,
                    StreamUpgradeError::Io(e) => {
                        self.pending_error = Some(StreamUpgradeError::Io(e));
                        return;
                    }
                    StreamUpgradeError::Apply(error) => match error {
                        outbound_hop::UpgradeError::Fatal(error) => {
                            self.pending_error =
                                Some(StreamUpgradeError::Apply(Either::Right(error)));
                            return;
                        }
                        outbound_hop::UpgradeError::ReservationFailed(error) => {
                            StreamUpgradeError::Apply(error)
                        }
                        outbound_hop::UpgradeError::CircuitFailed(_) => {
                            unreachable!("Do not emitt `CircuitFailed` for outgoing reservation.")
                        }
                    },
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
                    // Fatal error occured, thus handler is closing as quickly as possible.
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
            }
            OutboundOpenInfo::Connect { send_back } => {
                let non_fatal_error = match error {
                    StreamUpgradeError::Timeout => StreamUpgradeError::Timeout,
                    StreamUpgradeError::NegotiationFailed => StreamUpgradeError::NegotiationFailed,
                    StreamUpgradeError::Io(e) => {
                        self.pending_error = Some(StreamUpgradeError::Io(e));
                        return;
                    }
                    StreamUpgradeError::Apply(error) => match error {
                        outbound_hop::UpgradeError::Fatal(error) => {
                            self.pending_error =
                                Some(StreamUpgradeError::Apply(Either::Right(error)));
                            return;
                        }
                        outbound_hop::UpgradeError::CircuitFailed(error) => {
                            StreamUpgradeError::Apply(error)
                        }
                        outbound_hop::UpgradeError::ReservationFailed(_) => {
                            unreachable!("Do not emitt `ReservationFailed` for outgoing circuit.")
                        }
                    },
                };

                let _ = send_back.send(Err(()));

                self.queued_events
                    .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::OutboundCircuitReqFailed {
                            error: non_fatal_error,
                        },
                    ));
            }
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = In;
    type ToBehaviour = Event;
    type Error = StreamUpgradeError<
        Either<inbound_stop::FatalUpgradeError, outbound_hop::FatalUpgradeError>,
    >;
    type InboundProtocol = inbound_stop::Upgrade;
    type OutboundProtocol = outbound_hop::Upgrade;
    type OutboundOpenInfo = OutboundOpenInfo;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(inbound_stop::Upgrade {}, ())
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            In::Reserve { to_listener } => {
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            outbound_hop::Upgrade::Reserve,
                            OutboundOpenInfo::Reserve { to_listener },
                        ),
                    });
            }
            In::EstablishCircuit {
                send_back,
                dst_peer_id,
            } => {
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            outbound_hop::Upgrade::Connect { dst_peer_id },
                            OutboundOpenInfo::Connect { send_back },
                        ),
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

        // Return queued events.
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        if let Poll::Ready(Some(protocol)) = self.reservation.poll(cx) {
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest { protocol });
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
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                self.on_fully_negotiated_outbound(fully_negotiated_outbound)
            }
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            ConnectionEvent::AddressChange(_)
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
    ) -> Poll<Option<SubstreamProtocol<outbound_hop::Upgrade, OutboundOpenInfo>>> {
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
                    Poll::Ready(Some(SubstreamProtocol::new(
                        outbound_hop::Upgrade::Reserve,
                        OutboundOpenInfo::Reserve { to_listener },
                    ))),
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

pub enum OutboundOpenInfo {
    Reserve {
        to_listener: mpsc::Sender<transport::ToListenerMsg>,
    },
    Connect {
        send_back: oneshot::Sender<Result<super::Connection, ()>>,
    },
}
