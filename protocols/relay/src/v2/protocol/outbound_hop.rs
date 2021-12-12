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
use crate::v2::protocol::{HOP_PROTOCOL_NAME, MAX_MESSAGE_SIZE};
use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::{future::BoxFuture, prelude::*};
use futures_timer::Delay;
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_swarm::NegotiatedSubstream;
use prost::Message;
use std::convert::TryFrom;
use std::io::Cursor;
use std::iter;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use unsigned_varint::codec::UviBytes;

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

        let mut encoded_msg = Vec::new();
        msg.encode(&mut encoded_msg)
            .expect("Vec to have sufficient capacity.");

        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_MESSAGE_SIZE);
        let mut substream = Framed::new(substream, codec);

        async move {
            substream.send(std::io::Cursor::new(encoded_msg)).await?;
            let msg: bytes::BytesMut = substream
                .next()
                .await
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, ""))??;

            let HopMessage {
                r#type,
                peer: _,
                reservation,
                limit: _,
                status,
            } = HopMessage::decode(Cursor::new(msg))?;

            let r#type = hop_message::Type::from_i32(r#type).ok_or(UpgradeError::ParseTypeField)?;
            match r#type {
                hop_message::Type::Connect => return Err(UpgradeError::UnexpectedTypeConnect),
                hop_message::Type::Reserve => return Err(UpgradeError::UnexpectedTypeReserve),
                hop_message::Type::Status => {}
            }

            let status = Status::from_i32(status.ok_or(UpgradeError::MissingStatusField)?)
                .ok_or(UpgradeError::ParseStatusField)?;
            match status {
                Status::Ok => {}
                s => return Err(UpgradeError::UnexpectedStatus(s)),
            }

            let output = match self {
                Upgrade::Reserve => {
                    let reservation = reservation.ok_or(UpgradeError::MissingReservationField)?;

                    let addrs = if reservation.addrs.is_empty() {
                        return Err(UpgradeError::NoAddressesInReservation);
                    } else {
                        reservation
                            .addrs
                            .into_iter()
                            .map(TryFrom::try_from)
                            .collect::<Result<Vec<Multiaddr>, _>>()
                            .map_err(|_| UpgradeError::InvalidReservationAddrs)?
                    };

                    let renewal_timeout = if let Some(timestamp) = reservation.expire {
                        Some(
                            timestamp
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
                                .ok_or(UpgradeError::InvalidReservationExpiration)?,
                        )
                    } else {
                        None
                    };

                    substream.close().await?;

                    Output::Reservation {
                        renewal_timeout,
                        addrs,
                    }
                }
                Upgrade::Connect { .. } => {
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
    #[error("Failed to decode message: {0}.")]
    Decode(
        #[from]
        #[source]
        prost::DecodeError,
    ),
    #[error(transparent)]
    Io(#[from] std::io::Error),
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
        renewal_timeout: Option<Delay>,
        addrs: Vec<Multiaddr>,
    },
    Circuit {
        substream: NegotiatedSubstream,
        read_buffer: Bytes,
    },
}
