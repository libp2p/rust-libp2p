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
use libp2p_identity::PeerId;
use libp2p_swarm::Stream;
use thiserror::Error;

use crate::{proto, protocol::MAX_MESSAGE_SIZE, STOP_PROTOCOL_NAME};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Remote reported permission denied.")]
    PermissionDenied,
    #[error("Remote does not support the `{STOP_PROTOCOL_NAME}` protocol")]
    Unsupported,
    #[error("IO error")]
    Io(#[source] io::Error),
    #[error("Protocol error")]
    Protocol(#[from] ProtocolViolation),
}

impl Error {
    pub(crate) fn to_status(&self) -> proto::Status {
        match self {
            Error::ResourceLimitExceeded => proto::Status::RESOURCE_LIMIT_EXCEEDED,
            Error::PermissionDenied => proto::Status::PERMISSION_DENIED,
            Error::Unsupported => proto::Status::CONNECTION_FAILED,
            Error::Io(_) => proto::Status::CONNECTION_FAILED,
            Error::Protocol(
                ProtocolViolation::UnexpectedStatus(_) | ProtocolViolation::UnexpectedTypeConnect,
            ) => proto::Status::UNEXPECTED_MESSAGE,
            Error::Protocol(_) => proto::Status::MALFORMED_MESSAGE,
        }
    }
}

/// Depicts all forms of protocol violations.
#[derive(Debug, Error)]
pub enum ProtocolViolation {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("Expected 'status' field to be set.")]
    MissingStatusField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message status '{0:?}'")]
    UnexpectedStatus(proto::Status),
}

impl From<quick_protobuf_codec::Error> for Error {
    fn from(e: quick_protobuf_codec::Error) -> Self {
        Error::Protocol(ProtocolViolation::Codec(e))
    }
}

/// Attempts to _connect_ to a peer via the given stream.
pub(crate) async fn connect(
    io: Stream,
    src_peer_id: PeerId,
    max_duration: Duration,
    max_bytes: u64,
) -> Result<Circuit, Error> {
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

    substream.send(msg).await?;

    let proto::StopMessage {
        type_pb,
        peer: _,
        limit: _,
        status,
    } = substream
        .next()
        .await
        .ok_or(Error::Io(io::ErrorKind::UnexpectedEof.into()))??;

    match type_pb {
        proto::StopMessageType::CONNECT => {
            return Err(Error::Protocol(ProtocolViolation::UnexpectedTypeConnect))
        }
        proto::StopMessageType::STATUS => {}
    }

    match status {
        Some(proto::Status::OK) => {}
        Some(proto::Status::RESOURCE_LIMIT_EXCEEDED) => return Err(Error::ResourceLimitExceeded),
        Some(proto::Status::PERMISSION_DENIED) => return Err(Error::PermissionDenied),
        Some(s) => return Err(Error::Protocol(ProtocolViolation::UnexpectedStatus(s))),
        None => return Err(Error::Protocol(ProtocolViolation::MissingStatusField)),
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

    Ok(Circuit {
        dst_stream: io,
        dst_pending_data: read_buffer.freeze(),
    })
}

pub(crate) struct Circuit {
    pub(crate) dst_stream: Stream,
    pub(crate) dst_pending_data: Bytes,
}
