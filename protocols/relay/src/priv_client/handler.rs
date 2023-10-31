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
use crate::{priv_client, proto, HOP_PROTOCOL_NAME, STOP_PROTOCOL_NAME};
use either::Either;
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, FutureExt};
use futures::sink::SinkExt;
use futures::stream::{FuturesUnordered, StreamExt};
use futures_timer::Delay;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::upgrade::ReadyUpgrade;
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::handler::{
    ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol,
};
use log::debug;
use std::collections::VecDeque;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{fmt, io};

/// The maximum number of circuits being denied concurrently.
///
/// Circuits to be denied exceeding the limit are dropped.
const MAX_NUMBER_DENYING_CIRCUIT: usize = 8;
const DENYING_CIRCUIT_TIMEOUT: Duration = Duration::from_secs(60);

const MAX_CONCURRENT_STREAMS_PER_CONNECTION: usize = 10;
const STREAM_TIMEOUT: Duration = Duration::from_secs(60);

pub enum In {
    Reserve {
        to_listener: mpsc::Sender<transport::ToListenerMsg>,
    },
    EstablishCircuit {
        dst_peer_id: PeerId,
        to_dial: oneshot::Sender<Result<priv_client::Connection, outbound_hop::ConnectError>>,
    },
}

impl fmt::Debug for In {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            In::Reserve { to_listener: _ } => f.debug_struct("In::Reserve").finish(),
            In::EstablishCircuit {
                dst_peer_id,
                to_dial: _,
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
    /// An outbound circuit has been established.
    OutboundCircuitEstablished { limit: Option<protocol::Limit> },
    /// An inbound circuit has been established.
    InboundCircuitEstablished {
        src_peer_id: PeerId,
        limit: Option<protocol::Limit>,
    },
}

pub struct Handler {
    local_peer_id: PeerId,
    remote_peer_id: PeerId,
    remote_addr: Multiaddr,
    /// A pending fatal error that results in the connection being closed.
    pending_error: Option<
        StreamUpgradeError<
            Either<inbound_stop::ProtocolViolation, outbound_hop::ProtocolViolation>,
        >,
    >,

    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Handler as ConnectionHandler>::OutboundProtocol,
            <Handler as ConnectionHandler>::OutboundOpenInfo,
            <Handler as ConnectionHandler>::ToBehaviour,
            <Handler as ConnectionHandler>::Error,
        >,
    >,

    /// We issue a stream upgrade for each pending request.
    pending_requests: VecDeque<PendingRequest>,

    /// A `RESERVE` request is in-flight for each item in this queue.
    active_reserve_requests: VecDeque<mpsc::Sender<transport::ToListenerMsg>>,

    inflight_reserve_requests:
        futures_bounded::FuturesSet<Result<outbound_hop::Reservation, outbound_hop::ReserveError>>,

    /// A `CONNECT` request is in-flight for each item in this queue.
    active_connect_requests:
        VecDeque<oneshot::Sender<Result<priv_client::Connection, outbound_hop::ConnectError>>>,

    inflight_outbound_connect_requests:
        futures_bounded::FuturesSet<Result<outbound_hop::Circuit, outbound_hop::ConnectError>>,

    inflight_inbound_circuit_requests:
        futures_bounded::FuturesSet<Result<inbound_stop::Circuit, inbound_stop::Error>>,

    reservation: Reservation,

    /// Tracks substreams lent out to the transport.
    ///
    /// Contains a [`futures::future::Future`] for each lend out substream that
    /// resolves once the substream is dropped.
    alive_lend_out_substreams: FuturesUnordered<oneshot::Receiver<void::Void>>,

    circuit_deny_futs: futures_bounded::FuturesSet<Result<(), inbound_stop::Error>>,
}

impl Handler {
    pub fn new(local_peer_id: PeerId, remote_peer_id: PeerId, remote_addr: Multiaddr) -> Self {
        Self {
            local_peer_id,
            remote_peer_id,
            remote_addr,
            queued_events: Default::default(),
            pending_requests: Default::default(),
            active_reserve_requests: Default::default(),
            inflight_reserve_requests: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            inflight_outbound_connect_requests: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            active_connect_requests: Default::default(),
            pending_error: Default::default(),
            reservation: Reservation::None,
            alive_lend_out_substreams: Default::default(),
            inflight_inbound_circuit_requests: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            circuit_deny_futs: futures_bounded::FuturesSet::new(
                DENYING_CIRCUIT_TIMEOUT,
                MAX_NUMBER_DENYING_CIRCUIT,
            ),
        }
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, .. }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        let pending_request = self
            .pending_requests
            .pop_front()
            .expect("got a stream error without a pending request");

        match pending_request {
            PendingRequest::Reserve { mut to_listener } => {
                let error = match error {
                    StreamUpgradeError::Timeout => {
                        outbound_hop::ReserveError::Io(io::ErrorKind::TimedOut.into())
                    }
                    StreamUpgradeError::Apply(never) => void::unreachable(never),
                    StreamUpgradeError::NegotiationFailed => {
                        outbound_hop::ReserveError::Unsupported
                    }
                    StreamUpgradeError::Io(e) => outbound_hop::ReserveError::Io(e),
                };

                if let Err(e) =
                    to_listener.try_send(transport::ToListenerMsg::Reservation(Err(error)))
                {
                    log::debug!("Unable to send error to listener: {}", e.into_send_error())
                }
                self.reservation.failed();
            }
            PendingRequest::Connect {
                to_dial: send_back, ..
            } => {
                let error = match error {
                    StreamUpgradeError::Timeout => {
                        outbound_hop::ConnectError::Io(io::ErrorKind::TimedOut.into())
                    }
                    StreamUpgradeError::NegotiationFailed => {
                        outbound_hop::ConnectError::Unsupported
                    }
                    StreamUpgradeError::Io(e) => outbound_hop::ConnectError::Io(e),
                    StreamUpgradeError::Apply(v) => void::unreachable(v),
                };

                let _ = send_back.send(Err(error));
            }
        }
    }

    fn insert_to_deny_futs(&mut self, circuit: inbound_stop::Circuit) {
        let src_peer_id = circuit.src_peer_id();

        if self
            .circuit_deny_futs
            .try_push(circuit.deny(proto::Status::NO_RESERVATION))
            .is_err()
        {
            log::warn!(
                "Dropping existing inbound circuit request to be denied from {src_peer_id} in favor of new one."
            )
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = In;
    type ToBehaviour = Event;
    type Error = StreamUpgradeError<
        Either<inbound_stop::ProtocolViolation, outbound_hop::ProtocolViolation>,
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
                self.pending_requests
                    .push_back(PendingRequest::Reserve { to_listener });
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(ReadyUpgrade::new(HOP_PROTOCOL_NAME), ()),
                    });
            }
            In::EstablishCircuit {
                to_dial: send_back,
                dst_peer_id,
            } => {
                self.pending_requests.push_back(PendingRequest::Connect {
                    dst_peer_id,
                    to_dial: send_back,
                });
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(ReadyUpgrade::new(HOP_PROTOCOL_NAME), ()),
                    });
            }
        }
    }

    fn connection_keep_alive(&self) -> bool {
        self.reservation.is_some()
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
        loop {
            // Check for a pending (fatal) error.
            if let Some(err) = self.pending_error.take() {
                // The handler will not be polled again by the `Swarm`.
                return Poll::Ready(ConnectionHandlerEvent::Close(err));
            }

            debug_assert_eq!(
                self.inflight_reserve_requests.len(),
                self.active_reserve_requests.len(),
                "expect to have one active request per inflight stream"
            );

            // Reservations
            match self.inflight_reserve_requests.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(outbound_hop::Reservation {
                    renewal_timeout,
                    addrs,
                    limit,
                }))) => {
                    let to_listener = self
                        .active_reserve_requests
                        .pop_front()
                        .expect("must have active request for stream");

                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        self.reservation.accepted(
                            renewal_timeout,
                            addrs,
                            to_listener,
                            self.local_peer_id,
                            limit,
                        ),
                    ));
                }
                Poll::Ready(Ok(Err(error))) => {
                    let mut to_listener = self
                        .active_reserve_requests
                        .pop_front()
                        .expect("must have active request for stream");

                    if let Err(e) =
                        to_listener.try_send(transport::ToListenerMsg::Reservation(Err(error)))
                    {
                        log::debug!("Unable to send error to listener: {}", e.into_send_error())
                    }
                    self.reservation.failed();
                    continue;
                }
                Poll::Ready(Err(futures_bounded::Timeout { .. })) => {
                    let mut to_listener = self
                        .active_reserve_requests
                        .pop_front()
                        .expect("must have active request for stream");

                    if let Err(e) =
                        to_listener.try_send(transport::ToListenerMsg::Reservation(Err(
                            outbound_hop::ReserveError::Io(io::ErrorKind::TimedOut.into()),
                        )))
                    {
                        log::debug!("Unable to send error to listener: {}", e.into_send_error())
                    }
                    self.reservation.failed();
                    continue;
                }
                Poll::Pending => {}
            }

            debug_assert_eq!(
                self.inflight_outbound_connect_requests.len(),
                self.active_connect_requests.len(),
                "expect to have one active request per inflight stream"
            );

            // Circuits
            match self.inflight_outbound_connect_requests.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(outbound_hop::Circuit {
                    limit,
                    read_buffer,
                    stream,
                }))) => {
                    let to_listener = self
                        .active_connect_requests
                        .pop_front()
                        .expect("must have active request for stream");

                    let (tx, _) = oneshot::channel();

                    // TODO: What do we on error?
                    let _ = to_listener.send(Ok(priv_client::Connection {
                        state: priv_client::ConnectionState::new_outbound(stream, read_buffer, tx),
                    }));

                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::OutboundCircuitEstablished { limit },
                    ));
                }
                Poll::Ready(Ok(Err(error))) => {
                    let to_dialer = self
                        .active_connect_requests
                        .pop_front()
                        .expect("must have active request for stream");

                    let _ = to_dialer.send(Err(error));
                    continue;
                }
                Poll::Ready(Err(futures_bounded::Timeout { .. })) => {
                    let mut to_listener = self
                        .active_reserve_requests
                        .pop_front()
                        .expect("must have active request for stream");

                    if let Err(e) =
                        to_listener.try_send(transport::ToListenerMsg::Reservation(Err(
                            outbound_hop::ReserveError::Io(io::ErrorKind::TimedOut.into()),
                        )))
                    {
                        log::debug!("Unable to send error to listener: {}", e.into_send_error())
                    }
                    self.reservation.failed();
                    continue;
                }
                Poll::Pending => {}
            }

            // Return queued events.
            if let Some(event) = self.queued_events.pop_front() {
                return Poll::Ready(event);
            }

            match self.inflight_inbound_circuit_requests.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(circuit))) => match &mut self.reservation {
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
                        continue;
                    }
                },
                Poll::Ready(Ok(Err(e))) => {
                    log::debug!("An inbound circuit request failed: {e}");
                    continue;
                }
                Poll::Ready(Err(e)) => {
                    log::debug!("An inbound circuit request timed out: {e}");
                    continue;
                }
                Poll::Pending => {}
            }

            if let Poll::Ready(Some(to_listener)) = self.reservation.poll(cx) {
                self.pending_requests
                    .push_back(PendingRequest::Reserve { to_listener });

                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(ReadyUpgrade::new(HOP_PROTOCOL_NAME), ()),
                });
            }

            // Deny incoming circuit requests.
            match self.circuit_deny_futs.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(()))) => continue,
                Poll::Ready(Ok(Err(error))) => {
                    log::debug!("Denying inbound circuit failed: {error}");
                    continue;
                }
                Poll::Ready(Err(futures_bounded::Timeout { .. })) => {
                    log::debug!("Denying inbound circuit timed out");
                    continue;
                }
                Poll::Pending => {}
            }

            // Check status of lend out substreams.
            match self.alive_lend_out_substreams.poll_next_unpin(cx) {
                Poll::Ready(Some(Err(oneshot::Canceled))) => {}
                Poll::Ready(Some(Ok(v))) => void::unreachable(v),
                Poll::Ready(None) => continue,
                Poll::Pending => {}
            }

            return Poll::Pending;
        }
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
                if self
                    .inflight_inbound_circuit_requests
                    .try_push(inbound_stop::handle_open_circuit(stream))
                    .is_err()
                {
                    log::warn!("Dropping inbound stream because we are at capacity")
                }
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: stream,
                ..
            }) => {
                let pending_request = self.pending_requests.pop_front().expect(
                    "opened a stream without a pending connection command or a reserve listener",
                );
                match pending_request {
                    PendingRequest::Reserve { to_listener } => {
                        self.active_reserve_requests.push_back(to_listener);
                        if self
                            .inflight_reserve_requests
                            .try_push(outbound_hop::make_reservation(stream))
                            .is_err()
                        {
                            log::warn!("Dropping outbound stream because we are at capacity")
                        }
                    }
                    PendingRequest::Connect {
                        dst_peer_id,
                        to_dial: send_back,
                    } => {
                        self.active_connect_requests.push_back(send_back);

                        if self
                            .inflight_outbound_connect_requests
                            .try_push(outbound_hop::open_circuit(stream, dst_peer_id))
                            .is_err()
                        {
                            log::warn!("Dropping outbound stream because we are at capacity")
                        }
                    }
                }
            }
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                void::unreachable(listen_upgrade_error.error)
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

    fn is_some(&self) -> bool {
        matches!(self, Self::Accepted { .. } | Self::Renewing { .. })
    }

    /// Marks the current reservation as failed.
    fn failed(&mut self) {
        *self = Reservation::None;
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

pub(crate) enum PendingRequest {
    Reserve {
        /// A channel into the [`Transport`](priv_client::Transport).
        to_listener: mpsc::Sender<transport::ToListenerMsg>,
    },
    Connect {
        dst_peer_id: PeerId,
        /// A channel into the future returned by [`Transport::dial`](libp2p_core::Transport::dial).
        to_dial: oneshot::Sender<Result<priv_client::Connection, outbound_hop::ConnectError>>,
    },
}
