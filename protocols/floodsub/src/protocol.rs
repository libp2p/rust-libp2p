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
use crate::topic::TopicHash;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, PeerId, upgrade};
use protobuf::{ProtobufError, Message as ProtobufMessage};
use std::{error, fmt, io, iter};
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `ConnectionUpgrade` for the floodsub protocol.
#[derive(Debug, Clone, Default)]
pub struct FloodsubConfig {}

impl FloodsubConfig {
    /// Builds a new `FloodsubConfig`.
    #[inline]
    pub fn new() -> FloodsubConfig {
        FloodsubConfig {}
    }
}

impl UpgradeInfo for FloodsubConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    #[inline]
    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/floodsub/1.0.0")
    }
}

impl<TSocket> InboundUpgrade<TSocket> for FloodsubConfig
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = FloodsubRpc;
    type Error = FloodsubDecodeError;
    type Future = upgrade::ReadOneThen<upgrade::Negotiated<TSocket>, (), fn(Vec<u8>, ()) -> Result<FloodsubRpc, FloodsubDecodeError>>;

    #[inline]
    fn upgrade_inbound(self, socket: upgrade::Negotiated<TSocket>, _: Self::Info) -> Self::Future {
        upgrade::read_one_then(socket, 2048, (), |packet, ()| {
            let mut rpc: rpc_proto::RPC = protobuf::parse_from_bytes(&packet)?;

            let mut messages = Vec::with_capacity(rpc.get_publish().len());
            for mut publish in rpc.take_publish().into_iter() {
                messages.push(FloodsubMessage {
                    source: PeerId::from_bytes(publish.take_from()).map_err(|_| {
                        FloodsubDecodeError::InvalidPeerId
                    })?,
                    data: publish.take_data(),
                    sequence_number: publish.take_seqno(),
                    topics: publish
                        .take_topicIDs()
                        .into_iter()
                        .map(TopicHash::from_raw)
                        .collect(),
                });
            }

            Ok(FloodsubRpc {
                messages,
                subscriptions: rpc
                    .take_subscriptions()
                    .into_iter()
                    .map(|mut sub| FloodsubSubscription {
                        action: if sub.get_subscribe() {
                            FloodsubSubscriptionAction::Subscribe
                        } else {
                            FloodsubSubscriptionAction::Unsubscribe
                        },
                        topic: TopicHash::from_raw(sub.take_topicid()),
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
    ReadError(upgrade::ReadOneError),
    /// Error when decoding the raw buffer into a protobuf.
    ProtobufError(ProtobufError),
    /// Error when parsing the `PeerId` in the message.
    InvalidPeerId,
}

impl From<upgrade::ReadOneError> for FloodsubDecodeError {
    #[inline]
    fn from(err: upgrade::ReadOneError) -> Self {
        FloodsubDecodeError::ReadError(err)
    }
}

impl From<ProtobufError> for FloodsubDecodeError {
    #[inline]
    fn from(err: ProtobufError) -> Self {
        FloodsubDecodeError::ProtobufError(err)
    }
}

impl fmt::Display for FloodsubDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            FloodsubDecodeError::ReadError(ref err) =>
                write!(f, "Error while reading from socket: {}", err),
            FloodsubDecodeError::ProtobufError(ref err) =>
                write!(f, "Error while decoding protobuf: {}", err),
            FloodsubDecodeError::InvalidPeerId =>
                write!(f, "Error while decoding PeerId from message"),
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

    #[inline]
    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/floodsub/1.0.0")
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for FloodsubRpc
where
    TSocket: AsyncWrite + AsyncRead,
{
    type Output = ();
    type Error = io::Error;
    type Future = upgrade::WriteOne<upgrade::Negotiated<TSocket>>;

    #[inline]
    fn upgrade_outbound(self, socket: upgrade::Negotiated<TSocket>, _: Self::Info) -> Self::Future {
        let bytes = self.into_bytes();
        upgrade::write_one(socket, bytes)
    }
}

impl FloodsubRpc {
    /// Turns this `FloodsubRpc` into a message that can be sent to a substream.
    fn into_bytes(self) -> Vec<u8> {
        let mut proto = rpc_proto::RPC::new();

        for message in self.messages {
            let mut msg = rpc_proto::Message::new();
            msg.set_from(message.source.into_bytes());
            msg.set_data(message.data);
            msg.set_seqno(message.sequence_number);
            msg.set_topicIDs(
                message
                    .topics
                    .into_iter()
                    .map(TopicHash::into_string)
                    .collect(),
            );
            proto.mut_publish().push(msg);
        }

        for topic in self.subscriptions {
            let mut subscription = rpc_proto::RPC_SubOpts::new();
            subscription.set_subscribe(topic.action == FloodsubSubscriptionAction::Subscribe);
            subscription.set_topicid(topic.topic.into_string());
            proto.mut_subscriptions().push(subscription);
        }

        proto
            .write_to_bytes()
            .expect("there is no situation in which the protobuf message can be invalid")
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
    pub topics: Vec<TopicHash>,
}

/// A subscription received by the floodsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FloodsubSubscription {
    /// Action to perform.
    pub action: FloodsubSubscriptionAction,
    /// The topic from which to subscribe or unsubscribe.
    pub topic: TopicHash,
}

/// Action that a subscription wants to perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FloodsubSubscriptionAction {
    /// The remote wants to subscribe to the given topic.
    Subscribe,
    /// The remote wants to unsubscribe from the given topic.
    Unsubscribe,
}
