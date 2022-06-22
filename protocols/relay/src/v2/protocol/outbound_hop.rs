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

use crate::v2::message_proto::{hop_message, HopMessage, Peer, Status};
use crate::v2::protocol::{Limit, HOP_PROTOCOL_NAME, MAX_MESSAGE_SIZE};
use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::{future::BoxFuture, prelude::*};
use futures_timer::Delay;
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_swarm::NegotiatedSubstream;
use std::convert::TryFrom;
use std::iter;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub enum Upgrade {
    Reserve,
    Connect { dst_peer_id: PeerId },
}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(HOP_PROTOCOL_NAME)
    }
}

impl upgrade::OutboundUpgrade<NegotiatedSubstream> for Upgrade {
    type Output = Output;
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let msg = match self {
            Upgrade::Reserve => HopMessage {
                r#type: hop_message::Type::Reserve.into(),
                peer: None,
                reservation: None,
                limit: None,
                status: None,
            },
            Upgrade::Connect { dst_peer_id } => HopMessage {
                r#type: hop_message::Type::Connect.into(),
                peer: Some(Peer {
                    id: dst_peer_id.to_bytes(),
                    addrs: vec![],
                }),
                reservation: None,
                limit: None,
                status: None,
            },
        };

        let mut substream = Framed::new(substream, prost_codec::Codec::new(MAX_MESSAGE_SIZE));

        async move {
            substream.send(msg).await?;
            let HopMessage {
                r#type,
                peer: _,
                reservation,
                limit,
                status,
            } = substream
                .next()
                .await
                .ok_or(FatalUpgradeError::StreamClosed)??;

            let r#type =
                hop_message::Type::from_i32(r#type).ok_or(FatalUpgradeError::ParseTypeField)?;
            match r#type {
                hop_message::Type::Connect => {
                    return Err(FatalUpgradeError::UnexpectedTypeConnect.into())
                }
                hop_message::Type::Reserve => {
                    return Err(FatalUpgradeError::UnexpectedTypeReserve.into())
                }
                hop_message::Type::Status => {}
            }

            let status = Status::from_i32(status.ok_or(FatalUpgradeError::MissingStatusField)?)
                .ok_or(FatalUpgradeError::ParseStatusField)?;

            let limit = limit.map(Into::into);

            let output = match self {
                Upgrade::Reserve => {
                    match status {
                        Status::Ok => {}
                        Status::ReservationRefused => {
                            return Err(ReservationFailedReason::Refused.into())
                        }
                        Status::ResourceLimitExceeded => {
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
                        .map(TryFrom::try_from)
                        .collect::<Result<Vec<Multiaddr>, _>>()
                        .map_err(|_| FatalUpgradeError::InvalidReservationAddrs)?;

                    let renewal_timeout = reservation
                        .expire
                        .checked_sub(
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
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
                    match status {
                        Status::Ok => {}
                        Status::ResourceLimitExceeded => {
                            return Err(CircuitFailedReason::ResourceLimitExceeded.into())
                        }
                        Status::ConnectionFailed => {
                            return Err(CircuitFailedReason::ConnectionFailed.into())
                        }
                        Status::NoReservation => {
                            return Err(CircuitFailedReason::NoReservation.into())
                        }
                        Status::PermissionDenied => {
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

impl From<prost_codec::Error> for UpgradeError {
    fn from(error: prost_codec::Error) -> Self {
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
    #[error("Failed to encode or decode")]
    Codec(
        #[from]
        #[source]
        prost_codec::Error,
    ),
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
    UnexpectedStatus(Status),
}

pub enum Output {
    Reservation {
        renewal_timeout: Delay,
        addrs: Vec<Multiaddr>,
        limit: Option<Limit>,
    },
    Circuit {
        substream: NegotiatedSubstream,
        read_buffer: Bytes,
        limit: Option<Limit>,
    },
}
