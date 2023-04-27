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

use crate::proto;
use crate::protocol::{self, MAX_MESSAGE_SIZE, STOP_PROTOCOL_NAME};
use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::upgrade;
use libp2p_identity::PeerId;
use libp2p_swarm::{NegotiatedSubstream, StreamProtocol};
use std::iter;
use thiserror::Error;

pub struct Upgrade {}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(STOP_PROTOCOL_NAME)
    }
}

impl upgrade::InboundUpgrade<NegotiatedSubstream> for Upgrade {
    type Output = Circuit;
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let mut substream = Framed::new(
            substream,
            quick_protobuf_codec::Codec::new(MAX_MESSAGE_SIZE),
        );

        async move {
            let proto::StopMessage {
                type_pb,
                peer,
                limit,
                status: _,
            } = substream
                .next()
                .await
                .ok_or(FatalUpgradeError::StreamClosed)??;

            match type_pb {
                proto::StopMessageType::CONNECT => {
                    let src_peer_id =
                        PeerId::from_bytes(&peer.ok_or(FatalUpgradeError::MissingPeer)?.id)
                            .map_err(|_| FatalUpgradeError::ParsePeerId)?;
                    Ok(Circuit {
                        substream,
                        src_peer_id,
                        limit: limit.map(Into::into),
                    })
                }
                proto::StopMessageType::STATUS => {
                    Err(FatalUpgradeError::UnexpectedTypeStatus.into())
                }
            }
        }
        .boxed()
    }
}

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

pub struct Circuit {
    substream: Framed<NegotiatedSubstream, quick_protobuf_codec::Codec<proto::StopMessage>>,
    src_peer_id: PeerId,
    limit: Option<protocol::Limit>,
}

impl Circuit {
    pub fn src_peer_id(&self) -> PeerId {
        self.src_peer_id
    }

    pub fn limit(&self) -> Option<protocol::Limit> {
        self.limit
    }

    pub async fn accept(mut self) -> Result<(NegotiatedSubstream, Bytes), UpgradeError> {
        let msg = proto::StopMessage {
            type_pb: proto::StopMessageType::STATUS,
            peer: None,
            limit: None,
            status: Some(proto::Status::OK),
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

    pub async fn deny(mut self, status: proto::Status) -> Result<(), UpgradeError> {
        let msg = proto::StopMessage {
            type_pb: proto::StopMessageType::STATUS,
            peer: None,
            limit: None,
            status: Some(status),
        };

        self.send(msg).await.map_err(Into::into)
    }

    async fn send(&mut self, msg: proto::StopMessage) -> Result<(), quick_protobuf_codec::Error> {
        self.substream.send(msg).await?;
        self.substream.flush().await?;

        Ok(())
    }
}
