// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::rpc_proto;
use crate::topic::Topic;
use asynchronous_codec::Framed;
use futures::{
    io::{AsyncRead, AsyncWrite},
    Future,
};
use futures::{SinkExt, StreamExt};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use std::{io, iter, pin::Pin};

const MAX_MESSAGE_LEN_BYTES: usize = 2048;

const PROTOCOL_NAME: &[u8] = b"/floodsub/1.0.0";

/// Implementation of `ConnectionUpgrade` for the floodsub protocol.
#[derive(Debug, Clone, Default)]
pub struct FloodsubProtocol {}

impl FloodsubProtocol {
    /// Builds a new `FloodsubProtocol`.
    pub fn new() -> FloodsubProtocol {
        FloodsubProtocol {}
    }
}

impl UpgradeInfo for FloodsubProtocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<TSocket> InboundUpgrade<TSocket> for FloodsubProtocol
where
    TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = FloodsubRpc;
    type Error = FloodsubError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(
                socket,
                prost_codec::Codec::<rpc_proto::Rpc>::new(MAX_MESSAGE_LEN_BYTES),
            );

            let rpc = framed
                .next()
                .await
                .ok_or_else(|| FloodsubError::ReadError(io::ErrorKind::UnexpectedEof.into()))?
                .map_err(CodecError)?;

            let mut messages = Vec::with_capacity(rpc.publish.len());
            for publish in rpc.publish.into_iter() {
                messages.push(FloodsubMessage {
                    source: PeerId::from_bytes(&publish.from.unwrap_or_default())
                        .map_err(|_| FloodsubError::InvalidPeerId)?,
                    data: publish.data.unwrap_or_default(),
                    sequence_number: publish.seqno.unwrap_or_default(),
                    topics: publish.topic_ids.into_iter().map(Topic::new).collect(),
                });
            }

            Ok(FloodsubRpc {
                messages,
                subscriptions: rpc
                    .subscriptions
                    .into_iter()
                    .map(|sub| FloodsubSubscription {
                        action: if Some(true) == sub.subscribe {
                            FloodsubSubscriptionAction::Subscribe
                        } else {
                            FloodsubSubscriptionAction::Unsubscribe
                        },
                        topic: Topic::new(sub.topic_id.unwrap_or_default()),
                    })
                    .collect(),
            })
        })
    }
}

/// Reach attempt interrupt errors.
#[derive(thiserror::Error, Debug)]
pub enum FloodsubError {
    /// Error when parsing the `PeerId` in the message.
    #[error("Failed to decode PeerId from message")]
    InvalidPeerId,
    /// Error when decoding the raw buffer into a protobuf.
    #[error("Failed to decode protobuf")]
    ProtobufError(#[from] CodecError),
    /// Error when reading the packet from the socket.
    #[error("Failed to read from socket")]
    ReadError(#[from] io::Error),
}

#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct CodecError(#[from] prost_codec::Error);

/// An RPC received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloodsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<FloodsubMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<FloodsubSubscription>,
}

impl UpgradeInfo for FloodsubRpc {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for FloodsubRpc
where
    TSocket: AsyncWrite + AsyncRead + Send + Unpin + 'static,
{
    type Output = ();
    type Error = CodecError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(
                socket,
                prost_codec::Codec::<rpc_proto::Rpc>::new(MAX_MESSAGE_LEN_BYTES),
            );
            framed.send(self.into_rpc()).await?;
            framed.close().await?;
            Ok(())
        })
    }
}

impl FloodsubRpc {
    /// Turns this `FloodsubRpc` into a message that can be sent to a substream.
    fn into_rpc(self) -> rpc_proto::Rpc {
        rpc_proto::Rpc {
            publish: self
                .messages
                .into_iter()
                .map(|msg| rpc_proto::Message {
                    from: Some(msg.source.to_bytes()),
                    data: Some(msg.data),
                    seqno: Some(msg.sequence_number),
                    topic_ids: msg.topics.into_iter().map(|topic| topic.into()).collect(),
                })
                .collect(),

            subscriptions: self
                .subscriptions
                .into_iter()
                .map(|topic| rpc_proto::rpc::SubOpts {
                    subscribe: Some(topic.action == FloodsubSubscriptionAction::Subscribe),
                    topic_id: Some(topic.topic.into()),
                })
                .collect(),
        }
    }
}

/// A message received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloodsubMessage {
    /// Id of the peer that published this message.
    pub source: PeerId,

    /// Content of the message. Its meaning is out of scope of this library.
    pub data: Vec<u8>,

    /// An incrementing sequence number.
    pub sequence_number: Vec<u8>,

    /// List of topics this message belongs to.
    ///
    /// Each message can belong to multiple topics at once.
    pub topics: Vec<Topic>,
}

/// A subscription received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloodsubSubscription {
    /// Action to perform.
    pub action: FloodsubSubscriptionAction,
    /// The topic from which to subscribe or unsubscribe.
    pub topic: Topic,
}

/// Action that a subscription wants to perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FloodsubSubscriptionAction {
    /// The remote wants to subscribe to the given topic.
    Subscribe,
    /// The remote wants to unsubscribe from the given topic.
    Unsubscribe,
}
