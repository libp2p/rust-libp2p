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

//! A collection of types using the Gossipsub system.
use crate::TopicHash;
use libp2p_identity::PeerId;
use libp2p_swarm::ConnectionId;
use prometheus_client::encoding::EncodeLabelValue;
use quick_protobuf::MessageWrite;
use std::fmt;
use std::fmt::Debug;

use crate::rpc_proto::proto;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug)]
/// Validation kinds from the application for received messages.
pub enum MessageAcceptance {
    /// The message is considered valid, and it should be delivered and forwarded to the network.
    Accept,
    /// The message is considered invalid, and it should be rejected and trigger the P₄ penalty.
    Reject,
    /// The message is neither delivered nor forwarded to the network, but the router does not
    /// trigger the P₄ penalty.
    Ignore,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(pub Vec<u8>);

impl MessageId {
    pub fn new(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl<T: Into<Vec<u8>>> From<T> for MessageId {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex_fmt::HexFmt(&self.0))
    }
}

impl std::fmt::Debug for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MessageId({})", hex_fmt::HexFmt(&self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerConnections {
    /// The kind of protocol the peer supports.
    pub(crate) kind: PeerKind,
    /// Its current connections.
    pub(crate) connections: Vec<ConnectionId>,
}

/// Describes the types of peers that can exist in the gossipsub context.
#[derive(Debug, Clone, PartialEq, Hash, EncodeLabelValue, Eq)]
pub enum PeerKind {
    /// A gossipsub 1.1 peer.
    Gossipsubv1_1,
    /// A gossipsub 1.0 peer.
    Gossipsub,
    /// A floodsub peer.
    Floodsub,
    /// The peer doesn't support any of the protocols.
    NotSupported,
}

/// A message received by the gossipsub system and stored locally in caches..
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct RawMessage {
    /// Id of the peer that published this message.
    pub source: Option<PeerId>,

    /// Content of the message. Its meaning is out of scope of this library.
    pub data: Vec<u8>,

    /// A random sequence number.
    pub sequence_number: Option<u64>,

    /// The topic this message belongs to
    pub topic: TopicHash,

    /// The signature of the message if it's signed.
    pub signature: Option<Vec<u8>>,

    /// The public key of the message if it is signed and the source [`PeerId`] cannot be inlined.
    pub key: Option<Vec<u8>>,

    /// Flag indicating if this message has been validated by the application or not.
    pub validated: bool,
}

impl RawMessage {
    /// Calculates the encoded length of this message (used for calculating metrics).
    pub fn raw_protobuf_len(&self) -> usize {
        let message = proto::Message {
            from: self.source.map(|m| m.to_bytes()),
            data: Some(self.data.clone()),
            seqno: self.sequence_number.map(|s| s.to_be_bytes().to_vec()),
            topic: TopicHash::into_string(self.topic.clone()),
            signature: self.signature.clone(),
            key: self.key.clone(),
        };
        message.get_size()
    }
}

impl From<RawMessage> for proto::Message {
    fn from(raw: RawMessage) -> Self {
        proto::Message {
            from: raw.source.map(|m| m.to_bytes()),
            data: Some(raw.data),
            seqno: raw.sequence_number.map(|s| s.to_be_bytes().to_vec()),
            topic: TopicHash::into_string(raw.topic),
            signature: raw.signature,
            key: raw.key,
        }
    }
}

/// The message sent to the user after a [`RawMessage`] has been transformed by a
/// [`crate::DataTransform`].
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Message {
    /// Id of the peer that published this message.
    pub source: Option<PeerId>,

    /// Content of the message.
    pub data: Vec<u8>,

    /// A random sequence number.
    pub sequence_number: Option<u64>,

    /// The topic this message belongs to
    pub topic: TopicHash,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field(
                "data",
                &format_args!("{:<20}", &hex_fmt::HexFmt(&self.data)),
            )
            .field("source", &self.source)
            .field("sequence_number", &self.sequence_number)
            .field("topic", &self.topic)
            .finish()
    }
}

/// A subscription received by the gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Subscription {
    /// Action to perform.
    pub action: SubscriptionAction,
    /// The topic from which to subscribe or unsubscribe.
    pub topic_hash: TopicHash,
}

/// Action that a subscription wants to perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubscriptionAction {
    /// The remote wants to subscribe to the given topic.
    Subscribe,
    /// The remote wants to unsubscribe from the given topic.
    Unsubscribe,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerInfo {
    pub peer_id: Option<PeerId>,
    //TODO add this when RFC: Signed Address Records got added to the spec (see pull request
    // https://github.com/libp2p/specs/pull/217)
    //pub signed_peer_record: ?,
}

/// A Control message received by the gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ControlAction {
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
        /// A list of peers to be proposed to the removed peer as peer exchange
        peers: Vec<PeerInfo>,
        /// The backoff time in seconds before we allow to reconnect
        backoff: Option<u64>,
    },
}

/// A Gossipsub RPC message sent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RpcOut {
    /// Publish a Gossipsub message on network.
    Publish(RawMessage),
    /// Forward a Gossipsub message to the network.
    Forward(RawMessage),
    /// Subscribe a topic.
    Subscribe(TopicHash),
    /// Unsubscribe a topic.
    Unsubscribe(TopicHash),
    /// List of Gossipsub control messages.
    Control(ControlAction),
}

impl RpcOut {
    /// Converts the GossipsubRPC into its protobuf format.
    // A convenience function to avoid explicitly specifying types.
    pub fn into_protobuf(self) -> proto::RPC {
        self.into()
    }
}

impl From<RpcOut> for proto::RPC {
    /// Converts the RPC into protobuf format.
    fn from(rpc: RpcOut) -> Self {
        match rpc {
            RpcOut::Publish(message) => proto::RPC {
                subscriptions: Vec::new(),
                publish: vec![message.into()],
                control: None,
            },
            RpcOut::Forward(message) => proto::RPC {
                publish: vec![message.into()],
                subscriptions: Vec::new(),
                control: None,
            },
            RpcOut::Subscribe(topic) => proto::RPC {
                publish: Vec::new(),
                subscriptions: vec![proto::SubOpts {
                    subscribe: Some(true),
                    topic_id: Some(topic.into_string()),
                }],
                control: None,
            },
            RpcOut::Unsubscribe(topic) => proto::RPC {
                publish: Vec::new(),
                subscriptions: vec![proto::SubOpts {
                    subscribe: Some(false),
                    topic_id: Some(topic.into_string()),
                }],
                control: None,
            },
            RpcOut::Control(ControlAction::IHave {
                topic_hash,
                message_ids,
            }) => proto::RPC {
                publish: Vec::new(),
                subscriptions: Vec::new(),
                control: Some(proto::ControlMessage {
                    ihave: vec![proto::ControlIHave {
                        topic_id: Some(topic_hash.into_string()),
                        message_ids: message_ids.into_iter().map(|msg_id| msg_id.0).collect(),
                    }],
                    iwant: vec![],
                    graft: vec![],
                    prune: vec![],
                }),
            },
            RpcOut::Control(ControlAction::IWant { message_ids }) => proto::RPC {
                publish: Vec::new(),
                subscriptions: Vec::new(),
                control: Some(proto::ControlMessage {
                    ihave: vec![],
                    iwant: vec![proto::ControlIWant {
                        message_ids: message_ids.into_iter().map(|msg_id| msg_id.0).collect(),
                    }],
                    graft: vec![],
                    prune: vec![],
                }),
            },
            RpcOut::Control(ControlAction::Graft { topic_hash }) => proto::RPC {
                publish: Vec::new(),
                subscriptions: vec![],
                control: Some(proto::ControlMessage {
                    ihave: vec![],
                    iwant: vec![],
                    graft: vec![proto::ControlGraft {
                        topic_id: Some(topic_hash.into_string()),
                    }],
                    prune: vec![],
                }),
            },
            RpcOut::Control(ControlAction::Prune {
                topic_hash,
                peers,
                backoff,
            }) => {
                proto::RPC {
                    publish: Vec::new(),
                    subscriptions: vec![],
                    control: Some(proto::ControlMessage {
                        ihave: vec![],
                        iwant: vec![],
                        graft: vec![],
                        prune: vec![proto::ControlPrune {
                            topic_id: Some(topic_hash.into_string()),
                            peers: peers
                                .into_iter()
                                .map(|info| proto::PeerInfo {
                                    peer_id: info.peer_id.map(|id| id.to_bytes()),
                                    // TODO, see https://github.com/libp2p/specs/pull/217
                                    signed_peer_record: None,
                                })
                                .collect(),
                            backoff,
                        }],
                    }),
                }
            }
        }
    }
}

/// An RPC received/sent.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Rpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<RawMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<Subscription>,
    /// List of Gossipsub control messages.
    pub control_msgs: Vec<ControlAction>,
}

impl Rpc {
    /// Converts the GossipsubRPC into its protobuf format.
    // A convenience function to avoid explicitly specifying types.
    pub fn into_protobuf(self) -> proto::RPC {
        self.into()
    }
}

impl From<Rpc> for proto::RPC {
    /// Converts the RPC into protobuf format.
    fn from(rpc: Rpc) -> Self {
        // Messages
        let mut publish = Vec::new();

        for message in rpc.messages.into_iter() {
            let message = proto::Message {
                from: message.source.map(|m| m.to_bytes()),
                data: Some(message.data),
                seqno: message.sequence_number.map(|s| s.to_be_bytes().to_vec()),
                topic: TopicHash::into_string(message.topic),
                signature: message.signature,
                key: message.key,
            };

            publish.push(message);
        }

        // subscriptions
        let subscriptions = rpc
            .subscriptions
            .into_iter()
            .map(|sub| proto::SubOpts {
                subscribe: Some(sub.action == SubscriptionAction::Subscribe),
                topic_id: Some(sub.topic_hash.into_string()),
            })
            .collect::<Vec<_>>();

        // control messages
        let mut control = proto::ControlMessage {
            ihave: Vec::new(),
            iwant: Vec::new(),
            graft: Vec::new(),
            prune: Vec::new(),
        };

        let empty_control_msg = rpc.control_msgs.is_empty();

        for action in rpc.control_msgs {
            match action {
                // collect all ihave messages
                ControlAction::IHave {
                    topic_hash,
                    message_ids,
                } => {
                    let rpc_ihave = proto::ControlIHave {
                        topic_id: Some(topic_hash.into_string()),
                        message_ids: message_ids.into_iter().map(|msg_id| msg_id.0).collect(),
                    };
                    control.ihave.push(rpc_ihave);
                }
                ControlAction::IWant { message_ids } => {
                    let rpc_iwant = proto::ControlIWant {
                        message_ids: message_ids.into_iter().map(|msg_id| msg_id.0).collect(),
                    };
                    control.iwant.push(rpc_iwant);
                }
                ControlAction::Graft { topic_hash } => {
                    let rpc_graft = proto::ControlGraft {
                        topic_id: Some(topic_hash.into_string()),
                    };
                    control.graft.push(rpc_graft);
                }
                ControlAction::Prune {
                    topic_hash,
                    peers,
                    backoff,
                } => {
                    let rpc_prune = proto::ControlPrune {
                        topic_id: Some(topic_hash.into_string()),
                        peers: peers
                            .into_iter()
                            .map(|info| proto::PeerInfo {
                                peer_id: info.peer_id.map(|id| id.to_bytes()),
                                // TODO, see https://github.com/libp2p/specs/pull/217
                                signed_peer_record: None,
                            })
                            .collect(),
                        backoff,
                    };
                    control.prune.push(rpc_prune);
                }
            }
        }

        proto::RPC {
            subscriptions,
            publish,
            control: if empty_control_msg {
                None
            } else {
                Some(control)
            },
        }
    }
}

impl fmt::Debug for Rpc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut b = f.debug_struct("GossipsubRpc");
        if !self.messages.is_empty() {
            b.field("messages", &self.messages);
        }
        if !self.subscriptions.is_empty() {
            b.field("subscriptions", &self.subscriptions);
        }
        if !self.control_msgs.is_empty() {
            b.field("control_msgs", &self.control_msgs);
        }
        b.finish()
    }
}

impl PeerKind {
    pub fn as_static_ref(&self) -> &'static str {
        match self {
            Self::NotSupported => "Not Supported",
            Self::Floodsub => "Floodsub",
            Self::Gossipsub => "Gossipsub v1.0",
            Self::Gossipsubv1_1 => "Gossipsub v1.1",
        }
    }
}

impl AsRef<str> for PeerKind {
    fn as_ref(&self) -> &str {
        self.as_static_ref()
    }
}

impl fmt::Display for PeerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}
