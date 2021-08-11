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
use futures::{
    io::{AsyncRead, AsyncWrite},
    AsyncWriteExt, Future,
};
use libp2p_core::{upgrade, InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use prost::Message;
use std::{error, fmt, io, iter, pin::Pin};

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
        iter::once(b"/floodsub/1.0.0")
    }
}

impl<TSocket> InboundUpgrade<TSocket> for FloodsubProtocol
where
    TSocket: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = FloodsubRpc;
    type Error = FloodsubDecodeError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, mut socket: TSocket, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let packet = upgrade::read_length_prefixed(&mut socket, 2048).await?;
            let rpc = rpc_proto::Rpc::decode(&packet[..])?;

            let mut messages = Vec::with_capacity(rpc.publish.len());
            for publish in rpc.publish.into_iter() {
                messages.push(FloodsubMessage {
                    source: PeerId::from_bytes(&publish.from.unwrap_or_default())
                        .map_err(|_| FloodsubDecodeError::InvalidPeerId)?,
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
#[derive(Debug)]
pub enum FloodsubDecodeError {
    /// Error when reading the packet from the socket.
    ReadError(io::Error),
    /// Error when decoding the raw buffer into a protobuf.
    ProtobufError(prost::DecodeError),
    /// Error when parsing the `PeerId` in the message.
    InvalidPeerId,
}

impl From<io::Error> for FloodsubDecodeError {
    fn from(err: io::Error) -> Self {
        FloodsubDecodeError::ReadError(err)
    }
}

impl From<prost::DecodeError> for FloodsubDecodeError {
    fn from(err: prost::DecodeError) -> Self {
        FloodsubDecodeError::ProtobufError(err)
    }
}

impl fmt::Display for FloodsubDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            FloodsubDecodeError::ReadError(ref err) => {
                write!(f, "Error while reading from socket: {}", err)
            }
            FloodsubDecodeError::ProtobufError(ref err) => {
                write!(f, "Error while decoding protobuf: {}", err)
            }
            FloodsubDecodeError::InvalidPeerId => {
                write!(f, "Error while decoding PeerId from message")
            }
        }
    }
}

impl error::Error for FloodsubDecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            FloodsubDecodeError::ReadError(ref err) => Some(err),
            FloodsubDecodeError::ProtobufError(ref err) => Some(err),
            FloodsubDecodeError::InvalidPeerId => None,
        }
    }
}

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
        iter::once(b"/floodsub/1.0.0")
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for FloodsubRpc
where
    TSocket: AsyncWrite + AsyncRead + Send + Unpin + 'static,
{
    type Output = ();
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, mut socket: TSocket, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let bytes = self.into_bytes();

            upgrade::write_length_prefixed(&mut socket, bytes).await?;
            socket.close().await?;

            Ok(())
        })
    }
}

impl FloodsubRpc {
    /// Turns this `FloodsubRpc` into a message that can be sent to a substream.
    fn into_bytes(self) -> Vec<u8> {
        let rpc = rpc_proto::Rpc {
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
        };

        let mut buf = Vec::with_capacity(rpc.encoded_len());
        rpc.encode(&mut buf)
            .expect("Vec<u8> provides capacity as needed");
        buf
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
