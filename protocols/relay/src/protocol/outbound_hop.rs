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

use std::{io, time::Duration};

use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::prelude::*;
use futures_timer::Delay;
use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::Stream;
use thiserror::Error;
use web_time::SystemTime;

use crate::{
    HOP_PROTOCOL_NAME, proto,
    protocol::{Limit, MAX_MESSAGE_SIZE},
};

#[derive(Debug, Error)]
pub enum ConnectError {
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Relay failed to connect to destination.")]
    ConnectionFailed,
    #[error("Relay has no reservation for destination.")]
    NoReservation,
    #[error("Remote denied permission.")]
    PermissionDenied,
    #[error("Remote does not support the `{HOP_PROTOCOL_NAME}` protocol")]
    Unsupported,
    #[error("IO error")]
    Io(#[from] io::Error),
    #[error("Protocol error")]
    Protocol(#[from] ProtocolViolation),
}

#[derive(Debug, Error)]
pub enum ReserveError {
    #[error("Reservation refused.")]
    Refused,
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Remote does not support the `{HOP_PROTOCOL_NAME}` protocol")]
    Unsupported,
    #[error("IO error")]
    Io(#[from] io::Error),
    #[error("Protocol error")]
    Protocol(#[from] ProtocolViolation),
}

#[derive(Debug, Error)]
pub enum ProtocolViolation {
    #[error(transparent)]
    Codec(#[from] prost_codec::Error),
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
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'reserve'")]
    UnexpectedTypeReserve,
    #[error("Unexpected message type '{0}'")]
    UnexpectedType(String),
    #[error("Unexpected message status '{0}'")]
    UnexpectedStatus(String),
}

fn format_proto_status(code: i32) -> String {
    match proto::Status::try_from(code) {
        Ok(status) => status.as_str_name().to_string(),
        Err(_) => code.to_string(),
    }
}

impl From<prost_codec::Error> for ConnectError {
    fn from(e: prost_codec::Error) -> Self {
        ConnectError::Protocol(ProtocolViolation::Codec(e))
    }
}

impl From<prost_codec::Error> for ReserveError {
    fn from(e: prost_codec::Error) -> Self {
        ReserveError::Protocol(ProtocolViolation::Codec(e))
    }
}

pub(crate) struct Reservation {
    pub(crate) renewal_timeout: Delay,
    pub(crate) addrs: Vec<Multiaddr>,
    pub(crate) limit: Option<Limit>,
}

pub(crate) struct Circuit {
    pub(crate) stream: Stream,
    pub(crate) read_buffer: Bytes,
    pub(crate) limit: Option<Limit>,
}

pub(crate) async fn make_reservation(stream: Stream) -> Result<Reservation, ReserveError> {
    let msg = proto::HopMessage {
        r#type: proto::HopMessageType::Reserve as i32,
        peer: None,
        reservation: None,
        limit: None,
        status: None,
    };
    let mut substream = Framed::new(stream, prost_codec::Codec::new(MAX_MESSAGE_SIZE));

    substream.send(msg).await?;

    substream.close().await?;

    let proto::HopMessage {
        r#type,
        peer: _,
        reservation,
        limit,
        status,
    } = substream
        .next()
        .await
        .ok_or(ReserveError::Io(io::ErrorKind::UnexpectedEof.into()))??;

    let msg_type = proto::HopMessageType::try_from(r#type)
        .map_err(|_| ProtocolViolation::UnexpectedType(r#type.to_string()))?;

    match msg_type {
        proto::HopMessageType::Connect => {
            return Err(ReserveError::Protocol(
                ProtocolViolation::UnexpectedTypeConnect,
            ));
        }
        proto::HopMessageType::Reserve => {
            return Err(ReserveError::Protocol(
                ProtocolViolation::UnexpectedTypeReserve,
            ));
        }
        proto::HopMessageType::Status => {}
    }

    let limit = limit.map(Into::into);

    let status_code = status.ok_or(ProtocolViolation::MissingStatusField)?;
    match proto::Status::try_from(status_code) {
        Ok(proto::Status::Ok) => {}
        Ok(proto::Status::ReservationRefused) => {
            return Err(ReserveError::Refused);
        }
        Ok(proto::Status::ResourceLimitExceeded) => {
            return Err(ReserveError::ResourceLimitExceeded);
        }
        _ => {
            return Err(ReserveError::Protocol(ProtocolViolation::UnexpectedStatus(
                format_proto_status(status_code),
            )));
        }
    }

    let reservation = reservation.ok_or(ReserveError::Protocol(
        ProtocolViolation::MissingReservationField,
    ))?;

    if reservation.addrs.is_empty() {
        return Err(ReserveError::Protocol(
            ProtocolViolation::NoAddressesInReservation,
        ));
    }

    let addrs = reservation
        .addrs
        .into_iter()
        .map(|b| Multiaddr::try_from(b.to_vec()))
        .collect::<Result<Vec<Multiaddr>, _>>()
        .map_err(|_| ReserveError::Protocol(ProtocolViolation::InvalidReservationAddrs))?;

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
        .ok_or(ReserveError::Protocol(
            ProtocolViolation::InvalidReservationExpiration,
        ))?;

    Ok(Reservation {
        renewal_timeout,
        addrs,
        limit,
    })
}

pub(crate) async fn open_circuit(
    protocol: Stream,
    dst_peer_id: PeerId,
) -> Result<Circuit, ConnectError> {
    let msg = proto::HopMessage {
        r#type: proto::HopMessageType::Connect as i32,
        peer: Some(proto::Peer {
            id: dst_peer_id.to_bytes(),
            addrs: vec![],
        }),
        reservation: None,
        limit: None,
        status: None,
    };

    let mut substream = Framed::new(protocol, prost_codec::Codec::new(MAX_MESSAGE_SIZE));

    substream.send(msg).await?;

    let proto::HopMessage {
        r#type,
        peer: _,
        reservation: _,
        limit,
        status,
    } = substream
        .next()
        .await
        .ok_or(ConnectError::Io(io::ErrorKind::UnexpectedEof.into()))??;

    let msg_type = proto::HopMessageType::try_from(r#type)
        .map_err(|_| ProtocolViolation::UnexpectedType(r#type.to_string()))?;

    match msg_type {
        proto::HopMessageType::Connect => {
            return Err(ConnectError::Protocol(
                ProtocolViolation::UnexpectedTypeConnect,
            ));
        }
        proto::HopMessageType::Reserve => {
            return Err(ConnectError::Protocol(
                ProtocolViolation::UnexpectedTypeReserve,
            ));
        }
        proto::HopMessageType::Status => {}
    }

    match status {
        Some(s) => match proto::Status::try_from(s) {
            Ok(proto::Status::Ok) => {}
            Ok(proto::Status::ResourceLimitExceeded) => {
                return Err(ConnectError::ResourceLimitExceeded);
            }
            Ok(proto::Status::ConnectionFailed) => {
                return Err(ConnectError::ConnectionFailed);
            }
            Ok(proto::Status::NoReservation) => {
                return Err(ConnectError::NoReservation);
            }
            Ok(proto::Status::PermissionDenied) => {
                return Err(ConnectError::PermissionDenied);
            }
            _ => {
                return Err(ConnectError::Protocol(ProtocolViolation::UnexpectedStatus(
                    format_proto_status(s),
                )));
            }
        },
        None => {
            return Err(ConnectError::Protocol(
                ProtocolViolation::MissingStatusField,
            ));
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

    let circuit = Circuit {
        stream: io,
        read_buffer: read_buffer.freeze(),
        limit,
    };

    Ok(circuit)
}
