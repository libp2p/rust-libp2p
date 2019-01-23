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
use bytes::BytesMut;
use futures::{future, stream, Future, Stream};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, PeerId};
use protobuf::Message as ProtobufMessage;
use std::{io, iter};
use tokio_codec::{Decoder, FramedRead};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

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
    TSocket: AsyncRead,
{
    type Output = FloodsubRpc;
    type Error = io::Error;
    type Future = future::MapErr<future::AndThen<stream::StreamFuture<FramedRead<TSocket, FloodsubCodec>>, Result<FloodsubRpc, (io::Error, FramedRead<TSocket, FloodsubCodec>)>, fn((Option<FloodsubRpc>, FramedRead<TSocket, FloodsubCodec>)) -> Result<FloodsubRpc, (io::Error, FramedRead<TSocket, FloodsubCodec>)>>, fn((io::Error, FramedRead<TSocket, FloodsubCodec>)) -> io::Error>;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        FramedRead::new(socket, FloodsubCodec { length_prefix: Default::default() })
            .into_future()
            .and_then::<fn(_) -> _, _>(|(val, socket)| {
                val.ok_or_else(move || (io::ErrorKind::UnexpectedEof.into(), socket))
            })
            .map_err(|(err, _)| err)
    }
}

/// Implementation of `tokio_codec::Codec`.
pub struct FloodsubCodec {
    /// The codec for encoding/decoding the length prefix of messages.
    length_prefix: codec::UviBytes,
}

impl Decoder for FloodsubCodec {
    type Item = FloodsubRpc;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let packet = match self.length_prefix.decode(src)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let mut rpc: rpc_proto::RPC = protobuf::parse_from_bytes(&packet)?;

        let mut messages = Vec::with_capacity(rpc.get_publish().len());
        for mut publish in rpc.take_publish().into_iter() {
            messages.push(FloodsubMessage {
                source: PeerId::from_bytes(publish.take_from()).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "Invalid peer ID in message")
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

        Ok(Some(FloodsubRpc {
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
        }))
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
    TSocket: AsyncWrite,
{
    type Output = ();
    type Error = io::Error;
    type Future = future::Map<future::AndThen<tokio_io::io::WriteAll<TSocket, Vec<u8>>, tokio_io::io::Shutdown<TSocket>, fn((TSocket, Vec<u8>)) -> tokio_io::io::Shutdown<TSocket>>, fn(TSocket) -> ()>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let bytes = self.into_length_delimited_bytes();
        tokio_io::io::write_all(socket, bytes)
            .and_then::<fn(_) -> _, _>(|(socket, _)| tokio_io::io::shutdown(socket))
            .map(|_| ())
    }
}

impl FloodsubRpc {
    /// Turns this `FloodsubRpc` into a message that can be sent to a substream.
    fn into_length_delimited_bytes(self) -> Vec<u8> {
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
            .write_length_delimited_to_bytes()
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
