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
use std::convert::TryInto;
use std::iter;
use std::time::Duration;
use thiserror::Error;

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
        let mut msg = StopMessage::new();
        msg.set_type(stop_message::Type::CONNECT);
        msg.peer = protobuf::MessageField::some(Peer {
            id: Some(self.relay_peer_id.to_bytes()),
            addrs: vec![],
            ..Peer::default()
        });
        msg.limit = protobuf::MessageField::some(Limit {
            duration: Some(
                self.max_circuit_duration
                    .as_secs()
                    .try_into()
                    .expect("`max_circuit_duration` not to exceed `u32::MAX`."),
            ),
            data: Some(self.max_circuit_bytes),
            ..Limit::default()
        });

        let mut substream = Framed::new(substream, prost_codec::Codec::new(MAX_MESSAGE_SIZE));

        async move {
            substream.send(msg).await?;
            let StopMessage {
                type_,
                peer: _,
                limit: _,
                status,
                ..
            } = substream
                .next()
                .await
                .ok_or(FatalUpgradeError::StreamClosed)??;

            let ty = type_
                .ok_or(FatalUpgradeError::ParseTypeField)?
                .enum_value()
                .or(Err(FatalUpgradeError::ParseTypeField))?;

            match ty {
                stop_message::Type::CONNECT => {
                    return Err(FatalUpgradeError::UnexpectedTypeConnect.into())
                }
                stop_message::Type::STATUS => {}
            }

            let status = status
                .ok_or(FatalUpgradeError::MissingStatusField)?
                .enum_value()
                .or(Err(FatalUpgradeError::ParseStatusField))?;

            match status {
                Status::OK => {}
                Status::RESOURCE_LIMIT_EXCEEDED => {
                    return Err(CircuitFailedReason::ResourceLimitExceeded.into())
                }
                Status::PERMISSION_DENIED => {
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

impl From<prost_codec::Error> for UpgradeError {
    fn from(error: prost_codec::Error) -> Self {
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
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Failed to parse response type field.")]
    ParseStatusField,
    #[error("Unexpected message status '{0:?}'")]
    UnexpectedStatus(Status),
}
