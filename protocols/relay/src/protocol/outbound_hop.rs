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

use crate::proto;
use crate::protocol::{Limit, HOP_PROTOCOL_NAME, MAX_MESSAGE_SIZE};
use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::{future::BoxFuture, prelude::*};
use futures_timer::Delay;
use instant::{Duration, SystemTime};
use libp2p_core::{upgrade, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{Stream, StreamProtocol};
use std::convert::TryFrom;
use std::iter;
use thiserror::Error;

pub enum Upgrade {
    Reserve,
    Connect { dst_peer_id: PeerId },
}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(HOP_PROTOCOL_NAME)
    }
}

impl upgrade::OutboundUpgrade<Stream> for Upgrade {
    type Output = Output;
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, substream: Stream, _: Self::Info) -> Self::Future {
        let msg = match self {
            Upgrade::Reserve => proto::HopMessage {
                type_pb: proto::HopMessageType::RESERVE,
                peer: None,
                reservation: None,
                limit: None,
                status: None,
            },
            Upgrade::Connect { dst_peer_id } => proto::HopMessage {
                type_pb: proto::HopMessageType::CONNECT,
                peer: Some(proto::Peer {
                    id: dst_peer_id.to_bytes(),
                    addrs: vec![],
                }),
                reservation: None,
                limit: None,
                status: None,
            },
        };

        let mut substream = Framed::new(
            substream,
            quick_protobuf_codec::Codec::new(MAX_MESSAGE_SIZE),
        );

        async move {
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
                    return Err(FatalUpgradeError::UnexpectedTypeConnect.into())
                }
                proto::HopMessageType::RESERVE => {
                    return Err(FatalUpgradeError::UnexpectedTypeReserve.into())
                }
                proto::HopMessageType::STATUS => {}
            }

            let limit = limit.map(Into::into);

            let output = match self {
                Upgrade::Reserve => {
                    match status
                        .ok_or(UpgradeError::Fatal(FatalUpgradeError::MissingStatusField))?
                    {
                        proto::Status::OK => {}
                        proto::Status::RESERVATION_REFUSED => {
                            return Err(ReservationFailedReason::Refused.into())
                        }
                        proto::Status::RESOURCE_LIMIT_EXCEEDED => {
                            return Err(ReservationFailedReason::ResourceLimitExceeded.into())
                        }
                        s => return Err(FatalUpgradeError::UnexpectedStatus(s).into()),
                    }

                    let reservation =
                        reservation.ok_or(FatalUpgradeError::MissingReservationField)?;

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

                    Output::Reservation {
                        renewal_timeout,
                        addrs,
                        limit,
                    }
                }
                Upgrade::Connect { .. } => {
                    match status
                        .ok_or(UpgradeError::Fatal(FatalUpgradeError::MissingStatusField))?
                    {
                        proto::Status::OK => {}
                        proto::Status::RESOURCE_LIMIT_EXCEEDED => {
                            return Err(CircuitFailedReason::ResourceLimitExceeded.into())
                        }
                        proto::Status::CONNECTION_FAILED => {
                            return Err(CircuitFailedReason::ConnectionFailed.into())
                        }
                        proto::Status::NO_RESERVATION => {
                            return Err(CircuitFailedReason::NoReservation.into())
                        }
                        proto::Status::PERMISSION_DENIED => {
                            return Err(CircuitFailedReason::PermissionDenied.into())
                        }
                        s => return Err(FatalUpgradeError::UnexpectedStatus(s).into()),
                    }

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

                    Output::Circuit {
                        substream: io,
                        read_buffer: read_buffer.freeze(),
                        limit,
                    }
                }
            };

            Ok(output)
        }
        .boxed()
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
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

pub enum Output {
    Reservation {
        renewal_timeout: Delay,
        addrs: Vec<Multiaddr>,
        limit: Option<Limit>,
    },
    Circuit {
        substream: Stream,
        read_buffer: Bytes,
        limit: Option<Limit>,
    },
}
