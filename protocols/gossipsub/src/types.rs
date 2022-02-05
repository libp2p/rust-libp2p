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
use crate::rpc_proto;
use crate::TopicHash;
use libp2p_core::{connection::ConnectionId, PeerId};
use prometheus_client::encoding::text::Encode;
use prost::Message;
use std::fmt;
use std::fmt::Debug;

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

/// Macro for declaring message id types
macro_rules! declare_message_id_type {
    ($name: ident, $name_string: expr) => {
        #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub Vec<u8>);

        impl $name {
            pub fn new(value: &[u8]) -> Self {
                Self(value.to_vec())
            }
        }

        impl<T: Into<Vec<u8>>> From<T> for $name {
            fn from(value: T) -> Self {
                Self(value.into())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", hex_fmt::HexFmt(&self.0))
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", $name_string, hex_fmt::HexFmt(&self.0))
            }
        }
    };
}

// A type for gossipsub message ids.
declare_message_id_type!(MessageId, "MessageId");

// A type for gossipsub fast messsage ids, not to confuse with "real" message ids.
//
// A fast-message-id is an optional message_id that can be used to filter duplicates quickly. On
// high intensive networks with lots of messages, where the message_id is based on the result of
// decompressed traffic, it is beneficial to specify a `fast-message-id` that can identify and
// filter duplicates quickly without performing the overhead of decompression.
declare_message_id_type!(FastMessageId, "FastMessageId");

#[derive(Debug, Clone, PartialEq)]
pub struct PeerConnections {
    /// The kind of protocol the peer supports.
    pub kind: PeerKind,
    /// Its current connections.
    pub connections: Vec<ConnectionId>,
}

/// Describes the types of peers that can exist in the gossipsub context.
#[derive(Debug, Clone, PartialEq, Hash, Encode, Eq)]
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
pub struct RawGossipsubMessage {
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

impl RawGossipsubMessage {
    /// Calculates the encoded length of this message (used for calculating metrics).
    pub fn raw_protobuf_len(&self) -> usize {
        let message = rpc_proto::Message {
            from: self.source.map(|m| m.to_bytes()),
            data: Some(self.data.clone()),
            seqno: self.sequence_number.map(|s| s.to_be_bytes().to_vec()),
            topic: TopicHash::into_string(self.topic.clone()),
            signature: self.signature.clone(),
            key: self.key.clone(),
        };
        message.encoded_len()
    }
}

/// The message sent to the user after a [`RawGossipsubMessage`] has been transformed by a
/// [`crate::DataTransform`].
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GossipsubMessage {
    /// Id of the peer that published this message.
    pub source: Option<PeerId>,

    /// Content of the message.
    pub data: Vec<u8>,

    /// A random sequence number.
    pub sequence_number: Option<u64>,

    /// The topic this message belongs to
    pub topic: TopicHash,
}

impl fmt::Debug for GossipsubMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GossipsubMessage")
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerInfo {
    pub peer_id: Option<PeerId>,
    //TODO add this when RFC: Signed Address Records got added to the spec (see pull request
    // https://github.com/libp2p/specs/pull/217)
    //pub signed_peer_record: ?,
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
        /// A list of peers to be proposed to the removed peer as peer exchange
        peers: Vec<PeerInfo>,
        /// The backoff time in seconds before we allow to reconnect
        backoff: Option<u64>,
    },
}

/// An RPC received/sent.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GossipsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<RawGossipsubMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<GossipsubSubscription>,
    /// List of Gossipsub control messages.
    pub control_msgs: Vec<GossipsubControlAction>,
}

impl GossipsubRpc {
    /// Converts the GossipsubRPC into its protobuf format.
    // A convenience function to avoid explicitly specifying types.
    pub fn into_protobuf(self) -> rpc_proto::Rpc {
        self.into()
    }
}

impl From<GossipsubRpc> for rpc_proto::Rpc {
    /// Converts the RPC into protobuf format.
    fn from(rpc: GossipsubRpc) -> Self {
        // Messages
        let mut publish = Vec::new();

        for message in rpc.messages.into_iter() {
            let message = rpc_proto::Message {
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

        let empty_control_msg = rpc.control_msgs.is_empty();

        for action in rpc.control_msgs {
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
                GossipsubControlAction::Prune {
                    topic_hash,
                    peers,
                    backoff,
                } => {
                    let rpc_prune = rpc_proto::ControlPrune {
                        topic_id: Some(topic_hash.into_string()),
                        peers: peers
                            .into_iter()
                            .map(|info| rpc_proto::PeerInfo {
                                peer_id: info.peer_id.map(|id| id.to_bytes()),
                                /// TODO, see https://github.com/libp2p/specs/pull/217
                                signed_peer_record: None,
                            })
                            .collect(),
                        backoff,
                    };
                    control.prune.push(rpc_prune);
                }
            }
        }

        rpc_proto::Rpc {
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

impl fmt::Debug for GossipsubRpc {
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
