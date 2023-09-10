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

use asynchronous_codec::{Framed, FramedParts};
use either::Either;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures_timer::Delay;
use log::debug;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use void::Void;

use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionHandlerEvent, Stream, StreamUpgradeError};

use crate::priv_client::handler::ClientConnectionHandlerEvent;
use crate::priv_client::{handler, transport};
use crate::protocol::{Limit, MAX_MESSAGE_SIZE};
use crate::{priv_client, proto};

#[derive(Debug, Error)]
pub(crate) enum UpgradeError {
    #[error("Reservation failed")]
    ReservationFailed(#[from] ReservationFailedReason),
    #[error("Circuit failed")]
    CircuitFailed(#[from] CircuitFailedReason),
    #[error("Fatal")]
    Fatal(#[from] FatalUpgradeError),
}

impl From<quick_protobuf_codec::Error> for UpgradeError {
    fn from(error: quick_protobuf_codec::Error) -> Self {
        Self::Fatal(error.into())
    }
}

#[derive(Debug, Error)]
pub enum CircuitFailedReason {
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Relay failed to connect to destination.")]
    ConnectionFailed,
    #[error("Relay has no reservation for destination.")]
    NoReservation,
    #[error("Remote denied permission.")]
    PermissionDenied,
}

#[derive(Debug, Error)]
pub enum ReservationFailedReason {
    #[error("Reservation refused.")]
    Refused,
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
}

#[derive(Debug, Error)]
pub enum FatalUpgradeError {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("Stream closed")]
    StreamClosed,
    #[error("Expected 'status' field to be set.")]
    MissingStatusField,
    #[error("Expected 'reservation' field to be set.")]
    MissingReservationField,
    #[error("Expected at least one address in reservation.")]
    NoAddressesInReservation,
    #[error("Invalid expiration timestamp in reservation.")]
    InvalidReservationExpiration,
    #[error("Invalid addresses in reservation.")]
    InvalidReservationAddrs,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'reserve'")]
    UnexpectedTypeReserve,
    #[error("Failed to parse response type field.")]
    ParseStatusField,
    #[error("Unexpected message status '{0:?}'")]
    UnexpectedStatus(proto::Status),
}

pub(crate) struct Reservation {
    pub(crate) renewal_timeout: Delay,
    pub(crate) addrs: Vec<Multiaddr>,
    pub(crate) limit: Option<Limit>,
    pub(crate) to_listener: mpsc::Sender<transport::ToListenerMsg>,
}

pub(crate) async fn handle_reserve_message_response(
    protocol: Stream,
    to_listener: mpsc::Sender<transport::ToListenerMsg>,
) -> Result<Reservation, UpgradeError> {
    let msg = proto::HopMessage {
        type_pb: proto::HopMessageType::RESERVE,
        peer: None,
        reservation: None,
        limit: None,
        status: None,
    };
    let mut substream = Framed::new(protocol, quick_protobuf_codec::Codec::new(MAX_MESSAGE_SIZE));

    substream.send(msg).await?;

    let proto::HopMessage {
        type_pb,
        peer: _,
        reservation,
        limit,
        status,
    } = substream
        .next()
        .await
        .ok_or(FatalUpgradeError::StreamClosed)??;

    match type_pb {
        proto::HopMessageType::CONNECT => {
            return Err(FatalUpgradeError::UnexpectedTypeConnect.into());
        }
        proto::HopMessageType::RESERVE => {
            return Err(FatalUpgradeError::UnexpectedTypeReserve.into());
        }
        proto::HopMessageType::STATUS => {}
    }

    let limit = limit.map(Into::into);

    match status.ok_or(UpgradeError::Fatal(FatalUpgradeError::MissingStatusField))? {
        proto::Status::OK => {}
        proto::Status::RESERVATION_REFUSED => {
            return Err(ReservationFailedReason::Refused.into());
        }
        proto::Status::RESOURCE_LIMIT_EXCEEDED => {
            return Err(ReservationFailedReason::ResourceLimitExceeded.into());
        }
        s => return Err(FatalUpgradeError::UnexpectedStatus(s).into()),
    }

    let reservation = reservation.ok_or(FatalUpgradeError::MissingReservationField)?;

    if reservation.addrs.is_empty() {
        return Err(FatalUpgradeError::NoAddressesInReservation.into());
    }

    let addrs = reservation
        .addrs
        .into_iter()
        .map(|b| Multiaddr::try_from(b.to_vec()))
        .collect::<Result<Vec<Multiaddr>, _>>()
        .map_err(|_| FatalUpgradeError::InvalidReservationAddrs)?;

    let renewal_timeout = reservation
        .expire
        .checked_sub(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        // Renew the reservation after 3/4 of the reservation expiration timestamp.
        .and_then(|duration| duration.checked_sub(duration / 4))
        .map(Duration::from_secs)
        .map(Delay::new)
        .ok_or(FatalUpgradeError::InvalidReservationExpiration)?;

    substream.close().await?;

    Ok(Reservation {
        renewal_timeout,
        addrs,
        limit,
        to_listener,
    })
}

pub(crate) async fn handle_connection_message_response(
    protocol: Stream,
    remote_peer_id: PeerId,
    con_command: Command,
    tx: oneshot::Sender<Void>,
) -> Option<ClientConnectionHandlerEvent> {
    let msg = proto::HopMessage {
        type_pb: proto::HopMessageType::CONNECT,
        peer: Some(proto::Peer {
            id: con_command.dst_peer_id.to_bytes(),
            addrs: vec![],
        }),
        reservation: None,
        limit: None,
        status: None,
    };

    let mut substream = Framed::new(protocol, quick_protobuf_codec::Codec::new(MAX_MESSAGE_SIZE));

    if substream.send(msg).await.is_err() {
        return Some(ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(
            Either::Right(FatalUpgradeError::StreamClosed),
        )));
    }

    let proto::HopMessage {
        type_pb,
        peer: _,
        reservation: _,
        limit,
        status,
    } = match substream.next().await {
        Some(Ok(r)) => r,
        _ => {
            return Some(ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(
                Either::Right(FatalUpgradeError::StreamClosed),
            )))
        }
    };

    match type_pb {
        proto::HopMessageType::CONNECT => {
            return Some(ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(
                Either::Right(FatalUpgradeError::UnexpectedTypeConnect),
            )));
        }
        proto::HopMessageType::RESERVE => {
            return Some(ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(
                Either::Right(FatalUpgradeError::UnexpectedTypeReserve),
            )));
        }
        proto::HopMessageType::STATUS => {}
    }

    match status {
        Some(proto::Status::OK) => {}
        Some(proto::Status::RESOURCE_LIMIT_EXCEEDED) => {
            return Some(ConnectionHandlerEvent::NotifyBehaviour(
                handler::Event::OutboundCircuitReqFailed {
                    error: StreamUpgradeError::Apply(CircuitFailedReason::ResourceLimitExceeded),
                },
            ));
        }
        Some(proto::Status::CONNECTION_FAILED) => {
            return Some(ConnectionHandlerEvent::NotifyBehaviour(
                handler::Event::OutboundCircuitReqFailed {
                    error: StreamUpgradeError::Apply(CircuitFailedReason::ConnectionFailed),
                },
            ));
        }
        Some(proto::Status::NO_RESERVATION) => {
            return Some(ConnectionHandlerEvent::NotifyBehaviour(
                handler::Event::OutboundCircuitReqFailed {
                    error: StreamUpgradeError::Apply(CircuitFailedReason::NoReservation),
                },
            ));
        }
        Some(proto::Status::PERMISSION_DENIED) => {
            return Some(ConnectionHandlerEvent::NotifyBehaviour(
                handler::Event::OutboundCircuitReqFailed {
                    error: StreamUpgradeError::Apply(CircuitFailedReason::PermissionDenied),
                },
            ));
        }
        Some(s) => {
            return Some(ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(
                Either::Right(FatalUpgradeError::UnexpectedStatus(s)),
            )));
        }
        None => {
            return Some(ConnectionHandlerEvent::Close(StreamUpgradeError::Apply(
                Either::Right(FatalUpgradeError::MissingStatusField),
            )));
        }
    }

    let limit = limit.map(Into::into);

    let FramedParts {
        io,
        read_buffer,
        write_buffer,
        ..
    } = substream.into_parts();
    assert!(
        write_buffer.is_empty(),
        "Expect a flushed Framed to have empty write buffer."
    );

    match con_command.send_back.send(Ok(priv_client::Connection {
        state: priv_client::ConnectionState::new_outbound(io, read_buffer.freeze(), tx),
    })) {
        Ok(()) => Some(ConnectionHandlerEvent::NotifyBehaviour(
            handler::Event::OutboundCircuitEstablished { limit },
        )),
        Err(_) => {
            debug!(
                "Oneshot to `client::transport::Dial` future dropped. \
                         Dropping established relayed connection to {:?}.",
                remote_peer_id,
            );

            None
        }
    }
}

pub(crate) enum OutboundStreamInfo {
    Reserve(mpsc::Sender<transport::ToListenerMsg>),
    CircuitConnection(Command),
}

pub(crate) struct Command {
    dst_peer_id: PeerId,
    pub(crate) send_back: oneshot::Sender<Result<priv_client::Connection, ()>>,
}

impl Command {
    pub(crate) fn new(
        dst_peer_id: PeerId,
        send_back: oneshot::Sender<Result<priv_client::Connection, ()>>,
    ) -> Self {
        Self {
            dst_peer_id,
            send_back,
        }
    }
}
