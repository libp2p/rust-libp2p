// Copyright 2020 Sigma Prime Pty Ltd.
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

use crate::behaviour::GossipsubRpc;
use crate::rpc_proto;
use crate::topic::TopicHash;
use byteorder::{BigEndian, ByteOrder};
use bytes::Bytes;
use bytes::BytesMut;
use futures::future;
use futures::prelude::*;
use futures_codec::{Decoder, Encoder, Framed};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use prost::Message as ProtobufMessage;
use std::{borrow::Cow, fmt, io, iter, pin::Pin};
use unsigned_varint::codec;

/// Implementation of the `ConnectionUpgrade` for the Gossipsub protocol.
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    protocol_id: Cow<'static, [u8]>,
    max_transmit_size: usize,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            protocol_id: Cow::Borrowed(b"/meshsub/1.0.0"),
            max_transmit_size: 2048,
        }
    }
}

impl ProtocolConfig {
    /// Builds a new `ProtocolConfig`.
    /// Sets the maximum gossip transmission size.
    pub fn new(
        protocol_id: impl Into<Cow<'static, [u8]>>,
        max_transmit_size: usize,
    ) -> ProtocolConfig {
        ProtocolConfig {
            protocol_id: protocol_id.into(),
            max_transmit_size,
        }
    }
}

impl UpgradeInfo for ProtocolConfig {
    type Info = Cow<'static, [u8]>;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.protocol_id.clone())
    }
}

impl<TSocket> InboundUpgrade<TSocket> for ProtocolConfig
where
    TSocket: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = Framed<TSocket, GossipsubCodec>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let mut length_codec = codec::UviBytes::default();
        length_codec.set_max_len(self.max_transmit_size);
        Box::pin(future::ok(Framed::new(
            socket,
            GossipsubCodec { length_codec },
        )))
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for ProtocolConfig
where
    TSocket: AsyncWrite + AsyncRead + Unpin + Send + 'static,
{
    type Output = Framed<TSocket, GossipsubCodec>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let mut length_codec = codec::UviBytes::default();
        length_codec.set_max_len(self.max_transmit_size);
        Box::pin(future::ok(Framed::new(
            socket,
            GossipsubCodec { length_codec },
        )))
    }
}

/* Gossip codec for the framing */

pub struct GossipsubCodec {
    /// Codec to encode/decode the Unsigned varint length prefix of the frames.
    length_codec: codec::UviBytes,
}

impl Encoder for GossipsubCodec {
    type Item = GossipsubRpc;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // messages
        let publish = item
            .messages
            .into_iter()
            .map(|message| rpc_proto::Message {
                from: Some(message.source.into_bytes()),
                data: Some(message.data),
                seqno: Some(message.sequence_number.to_be_bytes().to_vec()),
                topic_ids: message
                    .topics
                    .into_iter()
                    .map(TopicHash::into_string)
                    .collect(),
            })
            .collect::<Vec<_>>();

        // subscriptions
        let subscriptions = item
            .subscriptions
            .into_iter()
            .map(|sub| rpc_proto::rpc::SubOpts {
                subscribe: Some(sub.action == GossipsubSubscriptionAction::Subscribe),
                topic_id: Some(sub.topic_hash.into_string()),
            })
            .collect::<Vec<_>>();

        // control messages
        let mut control = rpc_proto::ControlMessage {
            ihave: Vec::new(),
            iwant: Vec::new(),
            graft: Vec::new(),
            prune: Vec::new(),
        };

        let empty_control_msg = item.control_msgs.is_empty();

        for action in item.control_msgs {
            match action {
                // collect all ihave messages
                GossipsubControlAction::IHave {
                    topic_hash,
                    message_ids,
                } => {
                    let rpc_ihave = rpc_proto::ControlIHave {
                        topic_id: Some(topic_hash.into_string()),
                        message_ids: message_ids.into_iter().map(|msg_id| msg_id.0).collect(),
                    };
                    control.ihave.push(rpc_ihave);
                }
                GossipsubControlAction::IWant { message_ids } => {
                    let rpc_iwant = rpc_proto::ControlIWant {
                        message_ids: message_ids.into_iter().map(|msg_id| msg_id.0).collect(),
                    };
                    control.iwant.push(rpc_iwant);
                }
                GossipsubControlAction::Graft { topic_hash } => {
                    let rpc_graft = rpc_proto::ControlGraft {
                        topic_id: Some(topic_hash.into_string()),
                    };
                    control.graft.push(rpc_graft);
                }
                GossipsubControlAction::Prune { topic_hash } => {
                    let rpc_prune = rpc_proto::ControlPrune {
                        topic_id: Some(topic_hash.into_string()),
                    };
                    control.prune.push(rpc_prune);
                }
            }
        }

        let rpc = rpc_proto::Rpc {
            subscriptions,
            publish,
            control: if empty_control_msg {
                None
            } else {
                Some(control)
            },
        };

        let mut buf = Vec::with_capacity(rpc.encoded_len());

        rpc.encode(&mut buf)
            .expect("Buffer has sufficient capacity");

        // length prefix the protobuf message, ensuring the max limit is not hit
        self.length_codec.encode(Bytes::from(buf), dst)
    }
}

impl Decoder for GossipsubCodec {
    type Item = GossipsubRpc;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let packet = match self.length_codec.decode(src)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let rpc = rpc_proto::Rpc::decode(&packet[..])?;

        let mut messages = Vec::with_capacity(rpc.publish.len());
        for publish in rpc.publish.into_iter() {
            // ensure the sequence number is a u64
            let seq_no = publish.seqno.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sequence number was not provided",
                )
            })?;
            if seq_no.len() != 8 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sequence number has an incorrect size",
                ));
            }
            messages.push(GossipsubMessage {
                source: PeerId::from_bytes(publish.from.unwrap_or_default())
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid Peer Id"))?,
                data: publish.data.unwrap_or_default(),
                sequence_number: BigEndian::read_u64(&seq_no),
                topics: publish
                    .topic_ids
                    .into_iter()
                    .map(TopicHash::from_raw)
                    .collect(),
            });
        }

        let mut control_msgs = Vec::new();

        if let Some(rpc_control) = rpc.control {
            // Collect the gossipsub control messages
            let ihave_msgs: Vec<GossipsubControlAction> = rpc_control
                .ihave
                .into_iter()
                .map(|ihave| GossipsubControlAction::IHave {
                    topic_hash: TopicHash::from_raw(ihave.topic_id.unwrap_or_default()),
                    message_ids: ihave
                        .message_ids
                        .into_iter()
                        .map(|x| MessageId(x))
                        .collect::<Vec<_>>(),
                })
                .collect();

            let iwant_msgs: Vec<GossipsubControlAction> = rpc_control
                .iwant
                .into_iter()
                .map(|iwant| GossipsubControlAction::IWant {
                    message_ids: iwant
                        .message_ids
                        .into_iter()
                        .map(|x| MessageId(x))
                        .collect::<Vec<_>>(),
                })
                .collect();

            let graft_msgs: Vec<GossipsubControlAction> = rpc_control
                .graft
                .into_iter()
                .map(|graft| GossipsubControlAction::Graft {
                    topic_hash: TopicHash::from_raw(graft.topic_id.unwrap_or_default()),
                })
                .collect();

            let prune_msgs: Vec<GossipsubControlAction> = rpc_control
                .prune
                .into_iter()
                .map(|prune| GossipsubControlAction::Prune {
                    topic_hash: TopicHash::from_raw(prune.topic_id.unwrap_or_default()),
                })
                .collect();

            control_msgs.extend(ihave_msgs);
            control_msgs.extend(iwant_msgs);
            control_msgs.extend(graft_msgs);
            control_msgs.extend(prune_msgs);
        }

        Ok(Some(GossipsubRpc {
            messages,
            subscriptions: rpc
                .subscriptions
                .into_iter()
                .map(|sub| GossipsubSubscription {
                    action: if Some(true) == sub.subscribe {
                        GossipsubSubscriptionAction::Subscribe
                    } else {
                        GossipsubSubscriptionAction::Unsubscribe
                    },
                    topic_hash: TopicHash::from_raw(sub.topic_id.unwrap_or_default()),
                })
                .collect(),
            control_msgs,
        }))
    }
}

/// A type for gossipsub message ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Into<String> for MessageId {
    fn into(self) -> String {
        self.0.into()
    }
}

/// A message received by the gossipsub system.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GossipsubMessage {
    /// Id of the peer that published this message.
    pub source: PeerId,

    /// Content of the message. Its meaning is out of scope of this library.
    pub data: Vec<u8>,

    /// A random sequence number.
    pub sequence_number: u64,

    /// List of topics this message belongs to.
    ///
    /// Each message can belong to multiple topics at once.
    pub topics: Vec<TopicHash>,
}

impl fmt::Debug for GossipsubMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GossipsubMessage")
            .field("data",&format_args!("{:<20}", &hex_fmt::HexFmt(&self.data)))
            .field("source", &self.source)
            .field("sequence_number", &self.sequence_number)
            .field("topics", &self.topics)
            .finish()
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
        /// The topic of the messages.
        topic_hash: TopicHash,
        /// A list of known message ids (peer_id + sequence _number) as a string.
        message_ids: Vec<MessageId>,
    },
    /// The node requests specific message ids (peer_id + sequence _number) - IWant control message.
    IWant {
        /// A list of known message ids (peer_id + sequence _number) as a string.
        message_ids: Vec<MessageId>,
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
