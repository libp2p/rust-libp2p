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

use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::prelude::*;
use libp2p_identity::PeerId;
use libp2p_swarm::Stream;
use thiserror::Error;

use crate::{
    proto,
    protocol::{self, MAX_MESSAGE_SIZE},
};

pub(crate) async fn handle_open_circuit(io: Stream) -> Result<Circuit, Error> {
    let mut substream = Framed::new(io, prost_codec::Codec::new(MAX_MESSAGE_SIZE));

    let proto::StopMessage {
        r#type,
        peer,
        limit,
        status: _,
    } = substream
        .next()
        .await
        .ok_or(Error::Io(io::ErrorKind::UnexpectedEof.into()))??;

    let msg_type = proto::StopMessageType::try_from(r#type)
        .map_err(|_| ProtocolViolation::UnexpectedType(r#type.to_string()))?;

    match msg_type {
        proto::StopMessageType::Connect => {
            let src_peer_id = PeerId::from_bytes(&peer.ok_or(ProtocolViolation::MissingPeer)?.id)
                .map_err(|_| ProtocolViolation::ParsePeerId)?;
            Ok(Circuit {
                substream,
                src_peer_id,
                limit: limit.map(Into::into),
            })
        }
        proto::StopMessageType::Status => {
            Err(Error::Protocol(ProtocolViolation::UnexpectedTypeStatus))
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Protocol error")]
    Protocol(#[from] ProtocolViolation),
    #[error("IO error")]
    Io(#[from] io::Error),
}

impl From<prost_codec::Error> for Error {
    fn from(error: prost_codec::Error) -> Self {
        Self::Protocol(ProtocolViolation::Codec(error))
    }
}

#[derive(Debug, Error)]
pub(crate) enum ProtocolViolation {
    #[error(transparent)]
    Codec(#[from] prost_codec::Error),
    #[error("Failed to parse peer id.")]
    ParsePeerId,
    #[error("Expected 'peer' field to be set.")]
    MissingPeer,
    #[error("Unexpected message type 'status'")]
    UnexpectedTypeStatus,
    #[error("Unexpected message type '{0}'")]
    UnexpectedType(String),
}

pub(crate) struct Circuit {
    substream: Framed<Stream, prost_codec::Codec<proto::StopMessage>>,
    src_peer_id: PeerId,
    limit: Option<protocol::Limit>,
}

impl Circuit {
    pub(crate) fn src_peer_id(&self) -> PeerId {
        self.src_peer_id
    }

    pub(crate) fn limit(&self) -> Option<protocol::Limit> {
        self.limit
    }

    pub(crate) async fn accept(mut self) -> Result<(Stream, Bytes), Error> {
        let msg = proto::StopMessage {
            r#type: proto::StopMessageType::Status as i32,
            peer: None,
            limit: None,
            status: Some(proto::Status::Ok as i32),
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

    pub(crate) async fn deny(mut self, status: proto::Status) -> Result<(), Error> {
        let msg = proto::StopMessage {
            r#type: proto::StopMessageType::Status as i32,
            peer: None,
            limit: None,
            status: Some(status as i32),
        };

        self.send(msg).await?;

        Ok(())
    }

    async fn send(&mut self, msg: proto::StopMessage) -> Result<(), Error> {
        self.substream.send(msg).await?;
        self.substream.flush().await?;

        Ok(())
    }
}
