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
use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use futures::{future, stream, Future, Stream};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use libp2p_floodsub::TopicHash;
use protobuf::Message as ProtobufMessage;
use std::{io, iter};
use tokio_codec::{Decoder, FramedRead};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

/// Implementation of the `ConnectionUpgrade` for the Gossipsub protocol.
#[derive(Debug, Clone, Default)]
pub struct ProtocolConfig {}

impl ProtocolConfig {
    /// Builds a new `ProtocolConfig`.
    #[inline]
    pub fn new() -> ProtocolConfig {
        ProtocolConfig {}
    }
}

impl UpgradeInfo for ProtocolConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    #[inline]
    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/meshsub/1.0.0")
    }
}

impl<TSocket> InboundUpgrade<TSocket> for ProtocolConfig
where
    TSocket: AsyncRead,
{
    type Output = GossipsubRpc;
    type Error = io::Error;
    type Future = future::MapErr<
        future::AndThen<
            stream::StreamFuture<FramedRead<TSocket, GossipsubCodec>>,
            Result<GossipsubRpc, (io::Error, FramedRead<TSocket, GossipsubCodec>)>,
            fn(
                (Option<GossipsubRpc>, FramedRead<TSocket, GossipsubCodec>),
            )
                -> Result<GossipsubRpc, (io::Error, FramedRead<TSocket, GossipsubCodec>)>,
        >,
        fn((io::Error, FramedRead<TSocket, GossipsubCodec>)) -> io::Error,
    >;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        FramedRead::new(
            socket,
            GossipsubCodec {
                length_prefix: Default::default(),
            },
        )
        .into_future()
        .and_then::<fn(_) -> _, _>(|(val, socket)| {
            val.ok_or_else(move || (io::ErrorKind::UnexpectedEof.into(), socket))
        })
        .map_err(|(err, _)| err)
    }
}

/// Implementation of `tokio_codec::Codec`.
pub struct GossipsubCodec {
    /// The codec for encoding/decoding the length prefix of messages.
    length_prefix: codec::UviBytes,
}

impl Decoder for GossipsubCodec {
    type Item = GossipsubRpc;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let packet = match self.length_prefix.decode(src)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let mut rpc: rpc_proto::RPC = protobuf::parse_from_bytes(&packet)?;

        let mut messages = Vec::with_capacity(rpc.get_publish().len());
        for mut publish in rpc.take_publish().into_iter() {
            messages.push(GossipsubMessage {
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

        let mut rpc_control = rpc.take_control();
        let mut control_msgs = vec![];
        // Collect the gossipsub control messages
        let ihave_msgs: Vec<GossipsubControlAction> = rpc_control
            .take_ihave()
            .into_iter()
            .map(|mut ihave| {
                GossipsubControlAction::IHave {
                    topic_hash: TopicHash::from_raw(ihave.take_topicID()),
                    // TODO: Potentially format the message ids better
                    message_ids: ihave.take_messageIDs().into_vec(),
                }
            })
            .collect();

        let iwant_msgs: Vec<GossipsubControlAction> = rpc_control
            .take_iwant()
            .into_iter()
            .map(|mut iwant| {
                GossipsubControlAction::IWant {
                    // TODO: Potentially format the message ids better
                    message_ids: iwant.take_messageIDs().into_vec(),
                }
            })
            .collect();

        let graft_msgs: Vec<GossipsubControlAction> = rpc_control
            .take_graft()
            .into_iter()
            .map(|mut graft| GossipsubControlAction::Graft {
                topic_hash: TopicHash::from_raw(graft.take_topicID()),
            })
            .collect();

        let prune_msgs: Vec<GossipsubControlAction> = rpc_control
            .take_prune()
            .into_iter()
            .map(|mut prune| GossipsubControlAction::Prune {
                topic_hash: TopicHash::from_raw(prune.take_topicID()),
            })
            .collect();

        control_msgs.extend(ihave_msgs);
        control_msgs.extend(iwant_msgs);
        control_msgs.extend(graft_msgs);
        control_msgs.extend(prune_msgs);

        Ok(Some(GossipsubRpc {
            messages,
            subscriptions: rpc
                .take_subscriptions()
                .into_iter()
                .map(|mut sub| GossipsubSubscription {
                    action: if sub.get_subscribe() {
                        GossipsubSubscriptionAction::Subscribe
                    } else {
                        GossipsubSubscriptionAction::Unsubscribe
                    },
                    topic_hash: TopicHash::from_raw(sub.take_topicid()),
                })
                .collect(),
            control_msgs,
        }))
    }
}

/// An RPC received by the gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<GossipsubMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<GossipsubSubscription>,
    /// List of Gossipsub control messages.
    pub control_msgs: Vec<GossipsubControlAction>,
}

impl UpgradeInfo for GossipsubRpc {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    #[inline]
    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/meshsub/1.0.0")
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for GossipsubRpc
where
    TSocket: AsyncWrite,
{
    type Output = ();
    type Error = io::Error;
    type Future = future::Map<
        future::AndThen<
            tokio_io::io::WriteAll<TSocket, Vec<u8>>,
            tokio_io::io::Shutdown<TSocket>,
            fn((TSocket, Vec<u8>)) -> tokio_io::io::Shutdown<TSocket>,
        >,
        fn(TSocket) -> (),
    >;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let bytes = self.into_length_delimited_bytes();
        tokio_io::io::write_all(socket, bytes)
            .and_then::<fn(_) -> _, _>(|(socket, _)| tokio_io::io::shutdown(socket))
            .map(|_| ())
    }
}

impl GossipsubRpc {
    /// Turns this `GossipsubRpc` into a message that can be sent to a substream.
    fn into_length_delimited_bytes(self) -> Vec<u8> {
        let mut proto = rpc_proto::RPC::new();

        for message in self.messages.into_iter() {
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

        for subscription in self.subscriptions.into_iter() {
            let mut rpc_subscription = rpc_proto::RPC_SubOpts::new();
            rpc_subscription
                .set_subscribe(subscription.action == GossipsubSubscriptionAction::Subscribe);
            rpc_subscription.set_topicid(subscription.topic_hash.into_string());
            proto.mut_subscriptions().push(rpc_subscription);
        }

        // gossipsub control messages
        let mut control_msg = rpc_proto::ControlMessage::new();

        for action in self.control_msgs {
            match action {
                // collect all ihave messages
                GossipsubControlAction::IHave {
                    topic_hash,
                    message_ids,
                } => {
                    let mut rpc_ihave = rpc_proto::ControlIHave::new();
                    rpc_ihave.set_topicID(topic_hash.into_string());
                    for msg_id in message_ids {
                        rpc_ihave.mut_messageIDs().push(msg_id);
                    }
                    control_msg.mut_ihave().push(rpc_ihave);
                }
                GossipsubControlAction::IWant { message_ids } => {
                    let mut rpc_iwant = rpc_proto::ControlIWant::new();
                    for msg_id in message_ids {
                        rpc_iwant.mut_messageIDs().push(msg_id);
                    }
                    control_msg.mut_iwant().push(rpc_iwant);
                }
                GossipsubControlAction::Graft { topic_hash } => {
                    let mut rpc_graft = rpc_proto::ControlGraft::new();
                    rpc_graft.set_topicID(topic_hash.into_string());
                    control_msg.mut_graft().push(rpc_graft);
                }
                GossipsubControlAction::Prune { topic_hash } => {
                    let mut rpc_prune = rpc_proto::ControlPrune::new();
                    rpc_prune.set_topicID(topic_hash.into_string());
                    control_msg.mut_prune().push(rpc_prune);
                }
            }
        }

        proto.set_control(control_msg);

        proto
            .write_length_delimited_to_bytes()
            .expect("there is no situation in which the protobuf message can be invalid")
    }
}

/// A message received by the gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubMessage {
    /// Id of the peer that published this message.
    pub source: PeerId,

    /// Content of the message. Its meaning is out of scope of this library.
    pub data: Vec<u8>,

    /// A random sequence number.
    pub sequence_number: Vec<u8>,

    /// List of topics this message belongs to.
    ///
    /// Each message can belong to multiple topics at once.
    pub topics: Vec<TopicHash>,
}

impl GossipsubMessage {
    /// Converts message into a message_id.
    // To be compatible with the go implementation
    pub fn msg_id(&self) -> String {
        let mut source_string = self.source.to_base58();
        // the sequence number is a big endian uint64 (as per go implementation)
        // avoid a potential panic by setting the seqno to 0 if it is not long enough.
        // TODO: Check that this doesn't introduce a vulnerability or issue
        let mut seqno = 0;
        if self.sequence_number.len() >= 8 {
            seqno = BigEndian::read_u64(&self.sequence_number);
        }
        source_string.push_str(&seqno.to_string());
        source_string
    }
}

/// A subscription received by the gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubSubscription {
    /// Action to perform.
    pub action: GossipsubSubscriptionAction,
    /// The topic from which to subscribe or unsubscribe.
    pub topic_hash: TopicHash,
}

/// Action that a subscription wants to perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GossipsubSubscriptionAction {
    /// The remote wants to subscribe to the given topic.
    Subscribe,
    /// The remote wants to unsubscribe from the given topic.
    Unsubscribe,
}

/// A Control message received by the gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GossipsubControlAction {
    /// Node broadcasts known messages per topic - IHave control message.
    IHave {
        /// The topic of the message.
        topic_hash: TopicHash,
        /// A list of known message ids (peer_id + sequence _number) as a string.
        message_ids: Vec<String>,
    },
    /// The node requests specific message ids (peer_id + sequence _number) - IWant control message.
    IWant {
        /// A list of known message ids (peer_id + sequence _number) as a string.
        message_ids: Vec<String>,
    },
    /// The node has been added to the mesh - Graft control message.
    Graft {
        /// The mesh topic the peer should be added to.
        topic_hash: TopicHash,
    },
    /// The node has been removed from the mesh - Prune control message.
    Prune {
        /// The mesh topic the peer should be removed from.
        topic_hash: TopicHash,
    },
}
