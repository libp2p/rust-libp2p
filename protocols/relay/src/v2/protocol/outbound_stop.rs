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

use crate::v2::message_proto::{stop_message, Limit, Peer, Status, StopMessage};
use crate::v2::protocol::{MAX_MESSAGE_SIZE, STOP_PROTOCOL_NAME};
use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{upgrade, PeerId};
use libp2p_swarm::NegotiatedSubstream;
use prost::Message;
use std::convert::TryInto;
use std::io::Cursor;
use std::iter;
use std::time::Duration;
use thiserror::Error;
use unsigned_varint::codec::UviBytes;

pub struct Upgrade {
    pub relay_peer_id: PeerId,
    pub max_circuit_duration: Duration,
    pub max_circuit_bytes: u64,
}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(STOP_PROTOCOL_NAME)
    }
}

impl upgrade::OutboundUpgrade<NegotiatedSubstream> for Upgrade {
    type Output = (NegotiatedSubstream, Bytes);
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let msg = StopMessage {
            r#type: stop_message::Type::Connect.into(),
            peer: Some(Peer {
                id: self.relay_peer_id.to_bytes(),
                addrs: vec![],
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
            status: None,
        };

        let mut encoded_msg = Vec::with_capacity(msg.encoded_len());
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

            let StopMessage {
                r#type,
                peer: _,
                limit: _,
                status,
            } = StopMessage::decode(Cursor::new(msg))?;

            let r#type =
                stop_message::Type::from_i32(r#type).ok_or(FatalUpgradeError::ParseTypeField)?;
            match r#type {
                stop_message::Type::Connect => {
                    return Err(FatalUpgradeError::UnexpectedTypeConnect.into())
                }
                stop_message::Type::Status => {}
            }

            let status = Status::from_i32(status.ok_or(FatalUpgradeError::MissingStatusField)?)
                .ok_or(FatalUpgradeError::ParseStatusField)?;
            match status {
                Status::Ok => {}
                Status::ResourceLimitExceeded => {
                    return Err(CircuitFailedReason::ResourceLimitExceeded.into())
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
                "Expect a flushed Framed to have an empty write buffer."
            );

            Ok((io, read_buffer.freeze()))
        }
        .boxed()
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("Circuit failed")]
    CircuitFailed(#[from] CircuitFailedReason),
    #[error("Fatal")]
    Fatal(#[from] FatalUpgradeError),
}

impl From<std::io::Error> for UpgradeError {
    fn from(error: std::io::Error) -> Self {
        Self::Fatal(error.into())
    }
}

impl From<prost::DecodeError> for UpgradeError {
    fn from(error: prost::DecodeError) -> Self {
        Self::Fatal(error.into())
    }
}

#[derive(Debug, Error)]
pub enum CircuitFailedReason {
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Remote reported permission denied.")]
    PermissionDenied,
}

#[derive(Debug, Error)]
pub enum FatalUpgradeError {
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
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Failed to parse response type field.")]
    ParseStatusField,
    #[error("Unexpected message status '{0:?}'")]
    UnexpectedStatus(Status),
}
