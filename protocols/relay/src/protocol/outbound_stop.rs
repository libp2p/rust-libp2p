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

use std::io;
use std::time::Duration;

use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::prelude::*;
use thiserror::Error;

use libp2p_identity::PeerId;
use libp2p_swarm::Stream;

use crate::protocol::MAX_MESSAGE_SIZE;
use crate::{proto, STOP_PROTOCOL_NAME};

#[derive(Debug, Error)]
pub enum CircuitFailedReason {
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Remote reported permission denied.")]
    PermissionDenied,
    #[error("Remote does not support the `{STOP_PROTOCOL_NAME}` protocol")]
    Unsupported,
    #[error("IO error")]
    Io(#[source] io::Error),
}

#[derive(Debug, Error)]
pub enum FatalUpgradeError {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
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
    UnexpectedStatus(proto::Status),
}

/// Attempts to _connect_ to a peer via the given stream.
pub(crate) async fn connect(
    io: Stream,
    src_peer_id: PeerId,
    max_duration: Duration,
    max_bytes: u64,
) -> Result<Result<Circuit, CircuitFailed>, FatalUpgradeError> {
    let msg = proto::StopMessage {
        type_pb: proto::StopMessageType::CONNECT,
        peer: Some(proto::Peer {
            id: src_peer_id.to_bytes(),
            addrs: vec![],
        }),
        limit: Some(proto::Limit {
            duration: Some(
                max_duration
                    .as_secs()
                    .try_into()
                    .expect("`max_circuit_duration` not to exceed `u32::MAX`."),
            ),
            data: Some(max_bytes),
        }),
        status: None,
    };

    let mut substream = Framed::new(io, quick_protobuf_codec::Codec::new(MAX_MESSAGE_SIZE));

    if substream.send(msg).await.is_err() {
        return Err(FatalUpgradeError::StreamClosed);
    }

    let res = substream.next().await;

    if let None | Some(Err(_)) = res {
        return Err(FatalUpgradeError::StreamClosed);
    }

    let proto::StopMessage {
        type_pb,
        peer: _,
        limit: _,
        status,
    } = res.unwrap().expect("should be ok");

    match type_pb {
        proto::StopMessageType::CONNECT => return Err(FatalUpgradeError::UnexpectedTypeConnect),
        proto::StopMessageType::STATUS => {}
    }

    match status {
        Some(proto::Status::OK) => {}
        Some(proto::Status::RESOURCE_LIMIT_EXCEEDED) => {
            return Ok(Err(CircuitFailed {
                status: proto::Status::RESOURCE_LIMIT_EXCEEDED,
                error: CircuitFailedReason::ResourceLimitExceeded,
            }))
        }
        Some(proto::Status::PERMISSION_DENIED) => {
            return Ok(Err(CircuitFailed {
                status: proto::Status::PERMISSION_DENIED,
                error: CircuitFailedReason::PermissionDenied,
            }))
        }
        Some(s) => return Err(FatalUpgradeError::UnexpectedStatus(s)),
        None => return Err(FatalUpgradeError::MissingStatusField),
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

    Ok(Ok(Circuit {
        dst_stream: io,
        dst_pending_data: read_buffer.freeze(),
    }))
}

pub(crate) struct Circuit {
    pub(crate) dst_stream: Stream,
    pub(crate) dst_pending_data: Bytes,
}

pub(crate) struct CircuitFailed {
    pub(crate) status: proto::Status,
    pub(crate) error: CircuitFailedReason, // TODO: Remove this.
}
