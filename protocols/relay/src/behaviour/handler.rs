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
    collections::{HashMap, VecDeque},
    fmt, io,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use either::Either;
use futures::{
    future::{BoxFuture, FutureExt, TryFutureExt},
    io::AsyncWriteExt,
    stream::{FuturesUnordered, StreamExt},
};
use futures_timer::Delay;
use libp2p_core::{upgrade::ReadyUpgrade, ConnectedPoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound},
    ConnectionHandler, ConnectionHandlerEvent, ConnectionId, Stream, StreamProtocol,
    StreamUpgradeError, SubstreamProtocol,
};
use web_time::Instant;

use crate::{
    behaviour::CircuitId,
    copy_future::CopyFuture,
    proto,
    protocol::{inbound_hop, outbound_stop},
    HOP_PROTOCOL_NAME, STOP_PROTOCOL_NAME,
};

const MAX_CONCURRENT_STREAMS_PER_CONNECTION: usize = 10;
const STREAM_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct Config {
    pub reservation_duration: Duration,
    pub max_circuit_duration: Duration,
    pub max_circuit_bytes: u64,
}

pub enum In {
    AcceptReservationReq {
        inbound_reservation_req: inbound_hop::ReservationReq,
        addrs: Vec<Multiaddr>,
    },
    DenyReservationReq {
        inbound_reservation_req: inbound_hop::ReservationReq,
        status: proto::Status,
    },
    DenyCircuitReq {
        circuit_id: Option<CircuitId>,
        inbound_circuit_req: inbound_hop::CircuitReq,
        status: proto::Status,
    },
    NegotiateOutboundConnect {
        circuit_id: CircuitId,
        inbound_circuit_req: inbound_hop::CircuitReq,
        src_peer_id: PeerId,
        src_connection_id: ConnectionId,
    },
    AcceptAndDriveCircuit {
        circuit_id: CircuitId,
        dst_peer_id: PeerId,
        inbound_circuit_req: inbound_hop::CircuitReq,
        dst_stream: Stream,
        dst_pending_data: Bytes,
    },
}

impl fmt::Debug for In {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            In::AcceptReservationReq {
                inbound_reservation_req: _,
                addrs,
            } => f
                .debug_struct("In::AcceptReservationReq")
                .field("addrs", addrs)
                .finish(),
            In::DenyReservationReq {
                inbound_reservation_req: _,
                status,
            } => f
                .debug_struct("In::DenyReservationReq")
                .field("status", status)
                .finish(),
            In::DenyCircuitReq {
                circuit_id,
                inbound_circuit_req: _,
                status,
            } => f
                .debug_struct("In::DenyCircuitReq")
                .field("circuit_id", circuit_id)
                .field("status", status)
                .finish(),
            In::NegotiateOutboundConnect {
                circuit_id,
                inbound_circuit_req: _,
                src_peer_id,
                src_connection_id,
            } => f
                .debug_struct("In::NegotiateOutboundConnect")
                .field("circuit_id", circuit_id)
                .field("src_peer_id", src_peer_id)
                .field("src_connection_id", src_connection_id)
                .finish(),
            In::AcceptAndDriveCircuit {
                circuit_id,
                inbound_circuit_req: _,
                dst_peer_id,
                dst_stream: _,
                dst_pending_data: _,
            } => f
                .debug_struct("In::AcceptAndDriveCircuit")
                .field("circuit_id", circuit_id)
                .field("dst_peer_id", dst_peer_id)
                .finish(),
        }
    }
}

/// The events produced by the [`Handler`].
#[allow(clippy::large_enum_variant)]
pub enum Event {
    /// An inbound reservation request has been received.
    ReservationReqReceived {
        inbound_reservation_req: inbound_hop::ReservationReq,
        endpoint: ConnectedPoint,
        /// Indicates whether the request replaces an existing reservation.
        renewed: bool,
    },
    /// An inbound reservation request has been accepted.
    ReservationReqAccepted {
        /// Indicates whether the request replaces an existing reservation.
        renewed: bool,
    },
    /// Accepting an inbound reservation request failed.
    ReservationReqAcceptFailed { error: inbound_hop::Error },
    /// An inbound reservation request has been denied.
    ReservationReqDenied {},
    /// Denying an inbound reservation request has failed.
    ReservationReqDenyFailed { error: inbound_hop::Error },
    /// An inbound reservation has timed out.
    ReservationTimedOut {},
    /// An inbound circuit request has been received.
    CircuitReqReceived {
        inbound_circuit_req: inbound_hop::CircuitReq,
        endpoint: ConnectedPoint,
    },
    /// An inbound circuit request has been denied.
    CircuitReqDenied {
        circuit_id: Option<CircuitId>,
        dst_peer_id: PeerId,
    },
    /// Denying an inbound circuit request failed.
    CircuitReqDenyFailed {
        circuit_id: Option<CircuitId>,
        dst_peer_id: PeerId,
        error: inbound_hop::Error,
    },
    /// An inbound circuit request has been accepted.
    CircuitReqAccepted {
        circuit_id: CircuitId,
        dst_peer_id: PeerId,
    },
    /// Accepting an inbound circuit request failed.
    CircuitReqAcceptFailed {
        circuit_id: CircuitId,
        dst_peer_id: PeerId,
        error: inbound_hop::Error,
    },
    /// An outbound substream for an inbound circuit request has been
    /// negotiated.
    OutboundConnectNegotiated {
        circuit_id: CircuitId,
        src_peer_id: PeerId,
        src_connection_id: ConnectionId,
        inbound_circuit_req: inbound_hop::CircuitReq,
        dst_stream: Stream,
        dst_pending_data: Bytes,
    },
    /// Negotiating an outbound substream for an inbound circuit request failed.
    OutboundConnectNegotiationFailed {
        circuit_id: CircuitId,
        src_peer_id: PeerId,
        src_connection_id: ConnectionId,
        inbound_circuit_req: inbound_hop::CircuitReq,
        status: proto::Status,
        error: outbound_stop::Error,
    },
    /// An inbound circuit has closed.
    CircuitClosed {
        circuit_id: CircuitId,
        dst_peer_id: PeerId,
        error: Option<std::io::Error>,
    },
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::ReservationReqReceived {
                inbound_reservation_req: _,
                endpoint,
                renewed,
            } => f
                .debug_struct("Event::ReservationReqReceived")
                .field("endpoint", endpoint)
                .field("renewed", renewed)
                .finish(),
            Event::ReservationReqAccepted { renewed } => f
                .debug_struct("Event::ReservationReqAccepted")
                .field("renewed", renewed)
                .finish(),
            Event::ReservationReqAcceptFailed { error } => f
                .debug_struct("Event::ReservationReqAcceptFailed")
                .field("error", error)
                .finish(),
            Event::ReservationReqDenied {} => {
                f.debug_struct("Event::ReservationReqDenied").finish()
            }
            Event::ReservationReqDenyFailed { error } => f
                .debug_struct("Event::ReservationReqDenyFailed")
                .field("error", error)
                .finish(),
            Event::ReservationTimedOut {} => f.debug_struct("Event::ReservationTimedOut").finish(),
            Event::CircuitReqReceived {
                endpoint,
                inbound_circuit_req: _,
            } => f
                .debug_struct("Event::CircuitReqReceived")
                .field("endpoint", endpoint)
                .finish(),
            Event::CircuitReqDenied {
                circuit_id,
                dst_peer_id,
            } => f
                .debug_struct("Event::CircuitReqDenied")
                .field("circuit_id", circuit_id)
                .field("dst_peer_id", dst_peer_id)
                .finish(),
            Event::CircuitReqDenyFailed {
                circuit_id,
                dst_peer_id,
                error,
            } => f
                .debug_struct("Event::CircuitReqDenyFailed")
                .field("circuit_id", circuit_id)
                .field("dst_peer_id", dst_peer_id)
                .field("error", error)
                .finish(),
            Event::CircuitReqAccepted {
                circuit_id,
                dst_peer_id,
            } => f
                .debug_struct("Event::CircuitReqAccepted")
                .field("circuit_id", circuit_id)
                .field("dst_peer_id", dst_peer_id)
                .finish(),
            Event::CircuitReqAcceptFailed {
                circuit_id,
                dst_peer_id,
                error,
            } => f
                .debug_struct("Event::CircuitReqAcceptFailed")
                .field("circuit_id", circuit_id)
                .field("dst_peer_id", dst_peer_id)
                .field("error", error)
                .finish(),
            Event::OutboundConnectNegotiated {
                circuit_id,
                src_peer_id,
                src_connection_id,
                inbound_circuit_req: _,
                dst_stream: _,
                dst_pending_data: _,
            } => f
                .debug_struct("Event::OutboundConnectNegotiated")
                .field("circuit_id", circuit_id)
                .field("src_peer_id", src_peer_id)
                .field("src_connection_id", src_connection_id)
                .finish(),
            Event::OutboundConnectNegotiationFailed {
                circuit_id,
                src_peer_id,
                src_connection_id,
                inbound_circuit_req: _,
                status,
                error,
            } => f
                .debug_struct("Event::OutboundConnectNegotiationFailed")
                .field("circuit_id", circuit_id)
                .field("src_peer_id", src_peer_id)
                .field("src_connection_id", src_connection_id)
                .field("status", status)
                .field("error", error)
                .finish(),
            Event::CircuitClosed {
                circuit_id,
                dst_peer_id,
                error,
            } => f
                .debug_struct("Event::CircuitClosed")
                .field("circuit_id", circuit_id)
                .field("dst_peer_id", dst_peer_id)
                .field("error", error)
                .finish(),
        }
    }
}

/// [`ConnectionHandler`] that manages substreams for a relay on a single
/// connection with a peer.
pub struct Handler {
    endpoint: ConnectedPoint,

    /// Static [`Handler`] [`Config`].
    config: Config,

    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,

    /// The point in time when this connection started idleing.
    idle_at: Option<Instant>,

    /// Future handling inbound reservation request.
    reservation_request_future: Option<ReservationRequestFuture>,
    /// Timeout for the currently active reservation.
    active_reservation: Option<Delay>,

    /// Futures accepting an inbound circuit request.
    circuit_accept_futures: Futures<Result<CircuitParts, (CircuitId, PeerId, inbound_hop::Error)>>,
    /// Futures denying an inbound circuit request.
    circuit_deny_futures: Futures<(Option<CircuitId>, PeerId, Result<(), inbound_hop::Error>)>,
    /// Futures relaying data for circuit between two peers.
    circuits: Futures<(CircuitId, PeerId, Result<(), std::io::Error>)>,

    /// We issue a stream upgrade for each [`PendingConnect`] request.
    pending_connect_requests: VecDeque<PendingConnect>,

    /// A `CONNECT` request is in flight for these circuits.
    active_connect_requests: HashMap<CircuitId, PendingConnect>,

    inbound_workers: futures_bounded::FuturesSet<
        Result<Either<inbound_hop::ReservationReq, inbound_hop::CircuitReq>, inbound_hop::Error>,
    >,
    outbound_workers: futures_bounded::FuturesMap<
        CircuitId,
        Result<outbound_stop::Circuit, outbound_stop::Error>,
    >,
}

impl Handler {
    pub fn new(config: Config, endpoint: ConnectedPoint) -> Handler {
        Handler {
            inbound_workers: futures_bounded::FuturesSet::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            outbound_workers: futures_bounded::FuturesMap::new(
                STREAM_TIMEOUT,
                MAX_CONCURRENT_STREAMS_PER_CONNECTION,
            ),
            endpoint,
            config,
            queued_events: Default::default(),
            idle_at: None,
            reservation_request_future: Default::default(),
            circuit_accept_futures: Default::default(),
            circuit_deny_futures: Default::default(),
            circuits: Default::default(),
            active_reservation: Default::default(),
            pending_connect_requests: Default::default(),
            active_connect_requests: Default::default(),
        }
    }

    fn on_fully_negotiated_inbound(&mut self, stream: Stream) {
        if self
            .inbound_workers
            .try_push(inbound_hop::handle_inbound_request(
                stream,
                self.config.reservation_duration,
                self.config.max_circuit_duration,
                self.config.max_circuit_bytes,
            ))
            .is_err()
        {
            tracing::warn!("Dropping inbound stream because we are at capacity")
        }
    }

    fn on_fully_negotiated_outbound(&mut self, stream: Stream) {
        let connect = self
            .pending_connect_requests
            .pop_front()
            .expect("opened a stream without a pending stop command");

        if self
            .outbound_workers
            .try_push(
                connect.circuit_id,
                outbound_stop::connect(
                    stream,
                    connect.src_peer_id,
                    connect.max_circuit_duration,
                    connect.max_circuit_bytes,
                ),
            )
            .is_err()
        {
            tracing::warn!("Dropping outbound stream because we are at capacity")
        }

        self.active_connect_requests
            .insert(connect.circuit_id, connect);
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, .. }: DialUpgradeError<
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        let error = match error {
            StreamUpgradeError::Timeout => outbound_stop::Error::Io(io::ErrorKind::TimedOut.into()),
            StreamUpgradeError::NegotiationFailed => outbound_stop::Error::Unsupported,
            StreamUpgradeError::Io(e) => outbound_stop::Error::Io(e),
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            StreamUpgradeError::Apply(v) => libp2p_core::util::unreachable(v),
        };

        let stop_command = self
            .pending_connect_requests
            .pop_front()
            .expect("failed to open a stream without a pending stop command");

        self.queued_events
            .push_back(ConnectionHandlerEvent::NotifyBehaviour(
                Event::OutboundConnectNegotiationFailed {
                    circuit_id: stop_command.circuit_id,
                    src_peer_id: stop_command.src_peer_id,
                    src_connection_id: stop_command.src_connection_id,
                    inbound_circuit_req: stop_command.inbound_circuit_req,
                    status: proto::Status::CONNECTION_FAILED,
                    error,
                },
            ));
    }
}

enum ReservationRequestFuture {
    Accepting(BoxFuture<'static, Result<(), inbound_hop::Error>>),
    Denying(BoxFuture<'static, Result<(), inbound_hop::Error>>),
}

type Futures<T> = FuturesUnordered<BoxFuture<'static, T>>;

impl ConnectionHandler for Handler {
    type FromBehaviour = In;
    type ToBehaviour = Event;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(HOP_PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            In::AcceptReservationReq {
                inbound_reservation_req,
                addrs,
            } => {
                if self
                    .reservation_request_future
                    .replace(ReservationRequestFuture::Accepting(
                        inbound_reservation_req.accept(addrs).err_into().boxed(),
                    ))
                    .is_some()
                {
                    tracing::warn!("Dropping existing deny/accept future in favor of new one")
                }
            }
            In::DenyReservationReq {
                inbound_reservation_req,
                status,
            } => {
                if self
                    .reservation_request_future
                    .replace(ReservationRequestFuture::Denying(
                        inbound_reservation_req.deny(status).err_into().boxed(),
                    ))
                    .is_some()
                {
                    tracing::warn!("Dropping existing deny/accept future in favor of new one")
                }
            }
            In::NegotiateOutboundConnect {
                circuit_id,
                inbound_circuit_req,
                src_peer_id,
                src_connection_id,
            } => {
                self.pending_connect_requests.push_back(PendingConnect::new(
                    circuit_id,
                    inbound_circuit_req,
                    src_peer_id,
                    src_connection_id,
                    &self.config,
                ));
                self.queued_events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(ReadyUpgrade::new(STOP_PROTOCOL_NAME), ()),
                    });
            }
            In::DenyCircuitReq {
                circuit_id,
                inbound_circuit_req,
                status,
            } => {
                let dst_peer_id = inbound_circuit_req.dst();
                self.circuit_deny_futures.push(
                    inbound_circuit_req
                        .deny(status)
                        .err_into()
                        .map(move |result| (circuit_id, dst_peer_id, result))
                        .boxed(),
                );
            }
            In::AcceptAndDriveCircuit {
                circuit_id,
                dst_peer_id,
                inbound_circuit_req,
                dst_stream,
                dst_pending_data,
            } => {
                self.circuit_accept_futures.push(
                    inbound_circuit_req
                        .accept()
                        .err_into()
                        .map_ok(move |(src_stream, src_pending_data)| CircuitParts {
                            circuit_id,
                            src_stream,
                            src_pending_data,
                            dst_peer_id,
                            dst_stream,
                            dst_pending_data,
                        })
                        .map_err(move |e| (circuit_id, dst_peer_id, e))
                        .boxed(),
                );
            }
        }
    }

    fn connection_keep_alive(&self) -> bool {
        let Some(idle_at) = self.idle_at else {
            return true;
        };

        Instant::now().duration_since(idle_at) <= Duration::from_secs(10)
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        // Return queued events.
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        // Progress existing circuits.
        if let Poll::Ready(Some((circuit_id, dst_peer_id, result))) =
            self.circuits.poll_next_unpin(cx)
        {
            match result {
                Ok(()) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::CircuitClosed {
                            circuit_id,
                            dst_peer_id,
                            error: None,
                        },
                    ))
                }
                Err(e) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::CircuitClosed {
                            circuit_id,
                            dst_peer_id,
                            error: Some(e),
                        },
                    ))
                }
            }
        }

        // Process inbound protocol workers
        loop {
            match self.inbound_workers.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(Either::Left(inbound_reservation_req)))) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::ReservationReqReceived {
                            inbound_reservation_req,
                            endpoint: self.endpoint.clone(),
                            renewed: self.active_reservation.is_some(),
                        },
                    ));
                }
                Poll::Ready(Ok(Ok(Either::Right(inbound_circuit_req)))) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::CircuitReqReceived {
                            inbound_circuit_req,
                            endpoint: self.endpoint.clone(),
                        },
                    ));
                }
                Poll::Ready(Err(e)) => {
                    tracing::debug!("Inbound stream operation timed out: {e}");
                    continue;
                }
                Poll::Ready(Ok(Err(e))) => {
                    tracing::debug!("Inbound stream operation failed: {e}");
                    continue;
                }
                Poll::Pending => {
                    break;
                }
            }
        }

        // Process outbound protocol workers
        match self.outbound_workers.poll_unpin(cx) {
            Poll::Ready((id, Ok(Ok(circuit)))) => {
                let connect = self
                    .active_connect_requests
                    .remove(&id)
                    .expect("must have pending connect");

                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundConnectNegotiated {
                        circuit_id: id,
                        src_peer_id: connect.src_peer_id,
                        src_connection_id: connect.src_connection_id,
                        inbound_circuit_req: connect.inbound_circuit_req,
                        dst_stream: circuit.dst_stream,
                        dst_pending_data: circuit.dst_pending_data,
                    },
                ));
            }
            Poll::Ready((id, Ok(Err(error)))) => {
                let connect = self
                    .active_connect_requests
                    .remove(&id)
                    .expect("must have pending connect");

                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundConnectNegotiationFailed {
                        circuit_id: connect.circuit_id,
                        src_peer_id: connect.src_peer_id,
                        src_connection_id: connect.src_connection_id,
                        inbound_circuit_req: connect.inbound_circuit_req,
                        status: error.to_status(),
                        error,
                    },
                ));
            }
            Poll::Ready((id, Err(futures_bounded::Timeout { .. }))) => {
                let connect = self
                    .active_connect_requests
                    .remove(&id)
                    .expect("must have pending connect");

                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                    Event::OutboundConnectNegotiationFailed {
                        circuit_id: connect.circuit_id,
                        src_peer_id: connect.src_peer_id,
                        src_connection_id: connect.src_connection_id,
                        inbound_circuit_req: connect.inbound_circuit_req,
                        status: proto::Status::CONNECTION_FAILED, // Best fit?
                        error: outbound_stop::Error::Io(io::ErrorKind::TimedOut.into()),
                    },
                ));
            }
            Poll::Pending => {}
        }

        // Deny new circuits.
        if let Poll::Ready(Some((circuit_id, dst_peer_id, result))) =
            self.circuit_deny_futures.poll_next_unpin(cx)
        {
            match result {
                Ok(()) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::CircuitReqDenied {
                            circuit_id,
                            dst_peer_id,
                        },
                    ));
                }
                Err(error) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::CircuitReqDenyFailed {
                            circuit_id,
                            dst_peer_id,
                            error,
                        },
                    ));
                }
            }
        }

        // Accept new circuits.
        if let Poll::Ready(Some(result)) = self.circuit_accept_futures.poll_next_unpin(cx) {
            match result {
                Ok(parts) => {
                    let CircuitParts {
                        circuit_id,
                        mut src_stream,
                        src_pending_data,
                        dst_peer_id,
                        mut dst_stream,
                        dst_pending_data,
                    } = parts;
                    let max_circuit_duration = self.config.max_circuit_duration;
                    let max_circuit_bytes = self.config.max_circuit_bytes;

                    let circuit = async move {
                        let (result_1, result_2) = futures::future::join(
                            src_stream.write_all(&dst_pending_data),
                            dst_stream.write_all(&src_pending_data),
                        )
                        .await;
                        result_1?;
                        result_2?;

                        CopyFuture::new(
                            src_stream,
                            dst_stream,
                            max_circuit_duration,
                            max_circuit_bytes,
                        )
                        .await?;

                        Ok(())
                    }
                    .map(move |r| (circuit_id, dst_peer_id, r))
                    .boxed();

                    self.circuits.push(circuit);

                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::CircuitReqAccepted {
                            circuit_id,
                            dst_peer_id,
                        },
                    ));
                }
                Err((circuit_id, dst_peer_id, error)) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                        Event::CircuitReqAcceptFailed {
                            circuit_id,
                            dst_peer_id,
                            error,
                        },
                    ));
                }
            }
        }

        // Check active reservation.
        if let Some(Poll::Ready(())) = self
            .active_reservation
            .as_mut()
            .map(|fut| fut.poll_unpin(cx))
        {
            self.active_reservation = None;
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                Event::ReservationTimedOut {},
            ));
        }

        // Progress reservation request.
        match self.reservation_request_future.as_mut() {
            Some(ReservationRequestFuture::Accepting(fut)) => {
                if let Poll::Ready(result) = fut.poll_unpin(cx) {
                    self.reservation_request_future = None;

                    match result {
                        Ok(()) => {
                            let renewed = self
                                .active_reservation
                                .replace(Delay::new(self.config.reservation_duration))
                                .is_some();
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                                Event::ReservationReqAccepted { renewed },
                            ));
                        }
                        Err(error) => {
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                                Event::ReservationReqAcceptFailed { error },
                            ));
                        }
                    }
                }
            }
            Some(ReservationRequestFuture::Denying(fut)) => {
                if let Poll::Ready(result) = fut.poll_unpin(cx) {
                    self.reservation_request_future = None;

                    match result {
                        Ok(()) => {
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                                Event::ReservationReqDenied {},
                            ))
                        }
                        Err(error) => {
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                                Event::ReservationReqDenyFailed { error },
                            ));
                        }
                    }
                }
            }
            None => {}
        }

        // Check keep alive status.
        if self.active_reservation.is_none() {
            if self.idle_at.is_none() {
                self.idle_at = Some(Instant::now());
            }
        } else {
            self.idle_at = None;
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
                self.on_fully_negotiated_inbound(stream);
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: stream,
                ..
            }) => {
                self.on_fully_negotiated_outbound(stream);
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error);
            }
            _ => {}
        }
    }
}

struct CircuitParts {
    circuit_id: CircuitId,
    src_stream: Stream,
    src_pending_data: Bytes,
    dst_peer_id: PeerId,
    dst_stream: Stream,
    dst_pending_data: Bytes,
}

/// Holds everything we know about a to-be-issued `CONNECT` request to a peer.
struct PendingConnect {
    circuit_id: CircuitId,
    inbound_circuit_req: inbound_hop::CircuitReq,
    src_peer_id: PeerId,
    src_connection_id: ConnectionId,
    max_circuit_duration: Duration,
    max_circuit_bytes: u64,
}

impl PendingConnect {
    fn new(
        circuit_id: CircuitId,
        inbound_circuit_req: inbound_hop::CircuitReq,
        src_peer_id: PeerId,
        src_connection_id: ConnectionId,
        config: &Config,
    ) -> Self {
        Self {
            circuit_id,
            inbound_circuit_req,
            src_peer_id,
            src_connection_id,
            max_circuit_duration: config.max_circuit_duration,
            max_circuit_bytes: config.max_circuit_bytes,
        }
    }
}
