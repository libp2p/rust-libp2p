use crate::behaviour::GossipsubRpc;
use crate::rpc_proto;
use crate::topic::TopicHash;
use byteorder::{BigEndian, ByteOrder};
use bytes::Bytes;
use bytes::BytesMut;
use futures::future;
use libp2p_core::{upgrade, InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use protobuf::Message as ProtobufMessage;
use std::borrow::Cow;
use std::{io, iter};
use tokio_codec::{Decoder, Encoder, Framed};
use tokio_io::{AsyncRead, AsyncWrite};
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
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = Framed<upgrade::Negotiated<TSocket>, GossipsubCodec>;
    type Error = io::Error;
    type Future = future::FutureResult<Self::Output, Self::Error>;

    fn upgrade_inbound(self, socket: upgrade::Negotiated<TSocket>, _: Self::Info) -> Self::Future {
        let mut length_codec = codec::UviBytes::default();
        length_codec.set_max_len(self.max_transmit_size);
        future::ok(Framed::new(socket, GossipsubCodec { length_codec }))
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for ProtocolConfig
where
    TSocket: AsyncWrite + AsyncRead,
{
    type Output = Framed<upgrade::Negotiated<TSocket>, GossipsubCodec>;
    type Error = io::Error;
    type Future = future::FutureResult<Self::Output, Self::Error>;

    fn upgrade_outbound(self, socket: upgrade::Negotiated<TSocket>, _: Self::Info) -> Self::Future {
        let mut length_codec = codec::UviBytes::default();
        length_codec.set_max_len(self.max_transmit_size);
        future::ok(Framed::new(socket, GossipsubCodec { length_codec }))
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
        let mut proto = rpc_proto::RPC::new();

        for message in item.messages.into_iter() {
            let mut msg = rpc_proto::Message::new();
            msg.set_from(message.source.into_bytes());
            msg.set_data(message.data);
            msg.set_seqno(message.sequence_number.to_be_bytes().to_vec());
            msg.set_topicIDs(
                message
                    .topics
                    .into_iter()
                    .map(TopicHash::into_string)
                    .collect(),
            );
            proto.mut_publish().push(msg);
        }

        for subscription in item.subscriptions.into_iter() {
            let mut rpc_subscription = rpc_proto::RPC_SubOpts::new();
            rpc_subscription
                .set_subscribe(subscription.action == GossipsubSubscriptionAction::Subscribe);
            rpc_subscription.set_topicid(subscription.topic_hash.into_string());
            proto.mut_subscriptions().push(rpc_subscription);
        }

        // gossipsub control messages
        let mut control_msg = rpc_proto::ControlMessage::new();

        for action in item.control_msgs {
            match action {
                // collect all ihave messages
                GossipsubControlAction::IHave {
                    topic_hash,
                    message_ids,
                } => {
                    let mut rpc_ihave = rpc_proto::ControlIHave::new();
                    rpc_ihave.set_topicID(topic_hash.into_string());
                    for msg_id in message_ids {
                        rpc_ihave.mut_messageIDs().push(msg_id.0);
                    }
                    control_msg.mut_ihave().push(rpc_ihave);
                }
                GossipsubControlAction::IWant { message_ids } => {
                    let mut rpc_iwant = rpc_proto::ControlIWant::new();
                    for msg_id in message_ids {
                        rpc_iwant.mut_messageIDs().push(msg_id.0);
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

        let bytes = proto
            .write_to_bytes()
            .expect("there is no situation in which the protobuf message can be invalid");

        // length prefix the protobuf message, ensuring the max limit is not hit
        self.length_codec.encode(Bytes::from(bytes), dst)
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

        let mut rpc: rpc_proto::RPC = protobuf::parse_from_bytes(&packet)?;

        let mut messages = Vec::with_capacity(rpc.get_publish().len());
        for mut publish in rpc.take_publish().into_iter() {
            // ensure the sequence number is a u64
            let raw_seq = publish.take_seqno();
            if raw_seq.len() != 8 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sequence number has an incorrect size",
                ));
            }
            messages.push(GossipsubMessage {
                source: PeerId::from_bytes(publish.take_from())
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid Peer Id"))?,
                data: publish.take_data(),
                sequence_number: BigEndian::read_u64(&raw_seq),
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
            .map(|mut ihave| GossipsubControlAction::IHave {
                topic_hash: TopicHash::from_raw(ihave.take_topicID()),
                message_ids: ihave
                    .take_messageIDs()
                    .into_vec()
                    .into_iter()
                    .map(|x| MessageId(x))
                    .collect::<Vec<_>>(),
            })
            .collect();

        let iwant_msgs: Vec<GossipsubControlAction> = rpc_control
            .take_iwant()
            .into_iter()
            .map(|mut iwant| GossipsubControlAction::IWant {
                message_ids: iwant
                    .take_messageIDs()
                    .into_vec()
                    .into_iter()
                    .map(|x| MessageId(x))
                    .collect::<Vec<_>>(),
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
