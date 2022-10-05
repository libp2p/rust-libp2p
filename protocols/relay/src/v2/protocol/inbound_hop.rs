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

use crate::v2::message_proto::{hop_message, HopMessage, Limit, Reservation, Status};
use crate::v2::protocol::{HOP_PROTOCOL_NAME, MAX_MESSAGE_SIZE};
use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_swarm::NegotiatedSubstream;
use std::convert::TryInto;
use std::iter;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub struct Upgrade {
    pub reservation_duration: Duration,
    pub max_circuit_duration: Duration,
    pub max_circuit_bytes: u64,
}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(HOP_PROTOCOL_NAME)
    }
}

impl upgrade::InboundUpgrade<NegotiatedSubstream> for Upgrade {
    type Output = Req;
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let mut substream = Framed::new(substream, prost_codec::Codec::new(MAX_MESSAGE_SIZE));

        async move {
            let HopMessage {
                r#type,
                peer,
                reservation: _,
                limit: _,
                status: _,
            } = substream
                .next()
                .await
                .ok_or(FatalUpgradeError::StreamClosed)??;

            let r#type =
                hop_message::Type::from_i32(r#type).ok_or(FatalUpgradeError::ParseTypeField)?;
            let req = match r#type {
                hop_message::Type::Reserve => Req::Reserve(ReservationReq {
                    substream,
                    reservation_duration: self.reservation_duration,
                    max_circuit_duration: self.max_circuit_duration,
                    max_circuit_bytes: self.max_circuit_bytes,
                }),
                hop_message::Type::Connect => {
                    let dst = PeerId::from_bytes(&peer.ok_or(FatalUpgradeError::MissingPeer)?.id)
                        .map_err(|_| FatalUpgradeError::ParsePeerId)?;
                    Req::Connect(CircuitReq { dst, substream })
                }
                hop_message::Type::Status => {
                    return Err(FatalUpgradeError::UnexpectedTypeStatus.into())
                }
            };

            Ok(req)
        }
        .boxed()
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("Fatal")]
    Fatal(#[from] FatalUpgradeError),
}

impl From<prost_codec::Error> for UpgradeError {
    fn from(error: prost_codec::Error) -> Self {
        Self::Fatal(error.into())
    }
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
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Failed to parse peer id.")]
    ParsePeerId,
    #[error("Expected 'peer' field to be set.")]
    MissingPeer,
    #[error("Unexpected message type 'status'")]
    UnexpectedTypeStatus,
}

pub enum Req {
    Reserve(ReservationReq),
    Connect(CircuitReq),
}

pub struct ReservationReq {
    substream: Framed<NegotiatedSubstream, prost_codec::Codec<HopMessage>>,
    reservation_duration: Duration,
    max_circuit_duration: Duration,
    max_circuit_bytes: u64,
}

impl ReservationReq {
    pub async fn accept(self, addrs: Vec<Multiaddr>) -> Result<(), UpgradeError> {
        let msg = HopMessage {
            r#type: hop_message::Type::Status.into(),
            peer: None,
            reservation: Some(Reservation {
                addrs: addrs.into_iter().map(|a| a.to_vec()).collect(),
                expire: (SystemTime::now() + self.reservation_duration)
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                voucher: None,
            }),
            limit: Some(Limit {
                duration: Some(
                    self.max_circuit_duration
                        .as_secs()
                        .try_into()
                        .expect("`max_circuit_duration` not to exceed `u32::MAX`."),
                ),
                data: Some(self.max_circuit_bytes),
            }),
            status: Some(Status::Ok.into()),
        };

        self.send(msg).await
    }

    pub async fn deny(self, status: Status) -> Result<(), UpgradeError> {
        let msg = HopMessage {
            r#type: hop_message::Type::Status.into(),
            peer: None,
            reservation: None,
            limit: None,
            status: Some(status.into()),
        };

        self.send(msg).await
    }

    async fn send(mut self, msg: HopMessage) -> Result<(), UpgradeError> {
        self.substream.send(msg).await?;
        self.substream.flush().await?;
        self.substream.close().await?;

        Ok(())
    }
}

pub struct CircuitReq {
    dst: PeerId,
    substream: Framed<NegotiatedSubstream, prost_codec::Codec<HopMessage>>,
}

impl CircuitReq {
    pub fn dst(&self) -> PeerId {
        self.dst
    }

    pub async fn accept(mut self) -> Result<(NegotiatedSubstream, Bytes), UpgradeError> {
        let msg = HopMessage {
            r#type: hop_message::Type::Status.into(),
            peer: None,
            reservation: None,
            limit: None,
            status: Some(Status::Ok.into()),
        };

        self.send(msg).await?;

        let FramedParts {
            io,
            read_buffer,
            write_buffer,
            ..
        } = self.substream.into_parts();
        assert!(
            write_buffer.is_empty(),
            "Expect a flushed Framed to have an empty write buffer."
        );

        Ok((io, read_buffer.freeze()))
    }

    pub async fn deny(mut self, status: Status) -> Result<(), UpgradeError> {
        let msg = HopMessage {
            r#type: hop_message::Type::Status.into(),
            peer: None,
            reservation: None,
            limit: None,
            status: Some(status.into()),
        };
        self.send(msg).await?;
        self.substream.close().await.map_err(Into::into)
    }

    async fn send(&mut self, msg: HopMessage) -> Result<(), prost_codec::Error> {
        self.substream.send(msg).await?;
        self.substream.flush().await?;

        Ok(())
    }
}
