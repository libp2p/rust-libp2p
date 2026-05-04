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

use crate::{STOP_PROTOCOL_NAME, proto, protocol::MAX_MESSAGE_SIZE};

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
            Error::ResourceLimitExceeded => proto::Status::ResourceLimitExceeded,
            Error::PermissionDenied => proto::Status::PermissionDenied,
            Error::Unsupported => proto::Status::ConnectionFailed,
            Error::Io(_) => proto::Status::ConnectionFailed,
            Error::Protocol(
                ProtocolViolation::UnexpectedStatus(_) | ProtocolViolation::UnexpectedTypeConnect,
            ) => proto::Status::UnexpectedMessage,
            Error::Protocol(_) => proto::Status::MalformedMessage,
        }
    }
}

/// Depicts all forms of protocol violations.
#[derive(Debug, Error)]
pub enum ProtocolViolation {
    #[error(transparent)]
    Codec(#[from] prost_codec::Error),
    #[error("Expected 'status' field to be set.")]
    MissingStatusField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type '{0}'")]
    UnexpectedType(String),
    #[error("Unexpected message status '{0}'")]
    UnexpectedStatus(String),
}

impl From<prost_codec::Error> for Error {
    fn from(e: prost_codec::Error) -> Self {
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
        r#type: proto::StopMessageType::Connect as i32,
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

    let mut substream = Framed::new(io, prost_codec::Codec::new(MAX_MESSAGE_SIZE));

    substream.send(msg).await?;

    let proto::StopMessage {
        r#type,
        peer: _,
        limit: _,
        status,
    } = substream
        .next()
        .await
        .ok_or(Error::Io(io::ErrorKind::UnexpectedEof.into()))??;

    let msg_type = proto::StopMessageType::try_from(r#type)
        .map_err(|_| Error::Protocol(ProtocolViolation::UnexpectedType(r#type.to_string())))?;

    match msg_type {
        proto::StopMessageType::Connect => {
            return Err(Error::Protocol(ProtocolViolation::UnexpectedTypeConnect));
        }
        proto::StopMessageType::Status => {}
    }

    match status {
        Some(s) => match proto::Status::try_from(s) {
            Ok(proto::Status::Ok) => {}
            Ok(proto::Status::ResourceLimitExceeded) => return Err(Error::ResourceLimitExceeded),
            Ok(proto::Status::PermissionDenied) => return Err(Error::PermissionDenied),
            Ok(other) => {
                return Err(Error::Protocol(ProtocolViolation::UnexpectedStatus(
                    other.as_str_name().to_string(),
                )));
            }
            Err(_) => {
                return Err(Error::Protocol(ProtocolViolation::UnexpectedStatus(
                    s.to_string(),
                )));
            }
        },
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
