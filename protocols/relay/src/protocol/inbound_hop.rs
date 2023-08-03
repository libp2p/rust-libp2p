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

use std::time::{Duration, SystemTime};

use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::prelude::*;
use thiserror::Error;

use libp2p_core::Multiaddr;
use libp2p_identity::PeerId;
use libp2p_swarm::Stream;

use crate::proto;
use crate::proto::Status;

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("Fatal")]
    Fatal(#[from] FatalUpgradeError),
}

impl From<quick_protobuf_codec::Error> for UpgradeError {
    fn from(error: quick_protobuf_codec::Error) -> Self {
        Self::Fatal(error.into())
    }
}

#[derive(Debug, Error)]
pub enum FatalUpgradeError {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
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

pub struct ReservationReq {
    pub(crate) substream: Framed<Stream, quick_protobuf_codec::Codec<proto::HopMessage>>,
    pub(crate) reservation_duration: Duration,
    pub(crate) max_circuit_duration: Duration,
    pub(crate) max_circuit_bytes: u64,
}

impl ReservationReq {
    pub async fn accept(self, addrs: Vec<Multiaddr>) -> Result<(), UpgradeError> {
        if addrs.is_empty() {
            log::debug!(
                "Accepting relay reservation without providing external addresses of local node. \
                 Thus the remote node might not be able to advertise its relayed address."
            )
        }

        let msg = proto::HopMessage {
            type_pb: proto::HopMessageType::STATUS,
            peer: None,
            reservation: Some(proto::Reservation {
                addrs: addrs.into_iter().map(|a| a.to_vec()).collect(),
                expire: (SystemTime::now() + self.reservation_duration)
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                voucher: None,
            }),
            limit: Some(proto::Limit {
                duration: Some(
                    self.max_circuit_duration
                        .as_secs()
                        .try_into()
                        .expect("`max_circuit_duration` not to exceed `u32::MAX`."),
                ),
                data: Some(self.max_circuit_bytes),
            }),
            status: Some(proto::Status::OK),
        };

        self.send(msg).await
    }

    pub async fn deny(self, status: Status) -> Result<(), UpgradeError> {
        let msg = proto::HopMessage {
            type_pb: proto::HopMessageType::STATUS,
            peer: None,
            reservation: None,
            limit: None,
            status: Some(status),
        };

        self.send(msg).await
    }

    async fn send(mut self, msg: proto::HopMessage) -> Result<(), UpgradeError> {
        self.substream.send(msg).await?;
        self.substream.flush().await?;
        self.substream.close().await?;

        Ok(())
    }
}

pub struct CircuitReq {
    pub(crate) dst: PeerId,
    pub(crate) substream: Framed<Stream, quick_protobuf_codec::Codec<proto::HopMessage>>,
}

impl CircuitReq {
    pub fn dst(&self) -> PeerId {
        self.dst
    }

    pub async fn accept(mut self) -> Result<(Stream, Bytes), UpgradeError> {
        let msg = proto::HopMessage {
            type_pb: proto::HopMessageType::STATUS,
            peer: None,
            reservation: None,
            limit: None,
            status: Some(Status::OK),
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
        let msg = proto::HopMessage {
            type_pb: proto::HopMessageType::STATUS,
            peer: None,
            reservation: None,
            limit: None,
            status: Some(status),
        };
        self.send(msg).await?;
        self.substream.close().await.map_err(Into::into)
    }

    async fn send(&mut self, msg: proto::HopMessage) -> Result<(), quick_protobuf_codec::Error> {
        self.substream.send(msg).await?;
        self.substream.flush().await?;

        Ok(())
    }
}
