use libp2p_floodsub::TopicHash;
use libp2p_core::PeerId;
use rpc_proto;

/// Represents the hash of a `Message`.
///
/// Instead of a using the message as a whole, the API of floodsub uses a
/// hash of the message. You only have to build the hash once, then use it
/// everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageHash {
    hash: String,
}

impl MessageHash {
    /// Builds a new `MessageHash` from the given hash.
    #[inline]
    pub fn from_raw(hash: String) -> MessageHash {
        MessageHash { hash: hash }
    }

    /// Converts a `MessageHash` into a hash of the message as a `String`.
    #[inline]
    pub fn into_string(self) -> String {
        self.hash
    }
}


/// A message received by the Gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Message {
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

    /// To use for an authentication scheme (not yet defined or implemented),
    /// see rpc.proto for more info.
    pub signature: Vec<u8>,

    /// To use for an encryption scheme (not yet defined or implemented),
    /// see rpc.proto for more info.
    pub key: Vec<u8>,

    /// The hash of a `Message`.
    // Could be part of rpc.proto.
    pub hash: MessageHash,
}

impl Message {
    /// Returns the hash of the message.
    #[inline]
    pub fn hash(&self) -> &MessageHash {
        &self.hash
    }
}

impl AsRef<MessageHash> for Message {
    #[inline]
    fn as_ref(&self) -> &MessageHash {
        &self.hash
    }
}

impl From<Message> for MessageHash {
    #[inline]
    fn from(message: Message) -> MessageHash {
        message.hash
    }
}

impl<'a> From<&'a Message> for TopicHash {
    #[inline]
    fn from(message: &'a Message) -> MessageHash {
        message.hash.clone()
    }
}

/// Represents a message ID as a string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MessageIDStr {
    msg_id: String,
}

impl MessageIDStr {
    /// Builds a new `MessageIDStr` from the given string.
    #[inline]
    pub fn from_raw(str_id: String) -> MessageIDStr {
        MessageIDStr { msg_id: str_id }
    }

    /// Converts a `MessageIDStr` into a message ID as a `String`.
    #[inline]
    pub fn into_string(self) -> String {
        self.msg_id
    }
}

/// An ID to represent a message, which can be a hash of the message or a
/// message ID as a string. It is used to query for a message.
// This could actually be part of rpc.proto, saving the need to manually write
// this.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MessageID {
    /// The hash of the `Message`.
    pub hash: MessageHash,
    /// The `Message` ID.
    pub id: MessageIDStr,
}

impl MessageID {
    #[inline]
    pub fn new() -> MessageID {
        ::std::default::Default::default()
    }

    /// Convenience function that sets the `hash` field of `MessageID`
    /// with the provided `MessageHash` instance.
    pub fn set_msg_hash(&mut self, hash: MessageHash) {
        self.id = hash
    }

    /// Convenience function that sets the `id` field of `MessageID` with
    /// the provided `MessageIDStr` instance.
    pub fn set_msg_id(&mut self, id: MessageIDStr) {
        self.id = id
    }
}

impl AsRef<MessageHash> for MessageID {
    #[inline]
    fn as_ref(&self) -> &MessageHash {
        &self.hash
    }
}

impl From<MessageID> for MessageHash {
    #[inline]
    fn from(message: Message) -> MessageHash {
        message.hash
    }
}

impl<'a> From<&'a MessageID> for MessageHash {
    #[inline]
    fn from(message: &'a Message) -> MessageHash {
        message.hash.clone()
    }
}

impl AsRef<MessageIDStr> for MessageID {
    #[inline]
    fn as_ref(&self) -> &MessageIDStr {
        &self.id
    }
}

impl From<MessageID> for MessageIDStr {
    #[inline]
    fn from(message: MessageID) -> MessageIDStr {
        message.id
    }
}

impl<'a> From<&'a MessageID> for MessageIDStr {
    #[inline]
    fn from(message: &'a Message) -> MessageIDStr {
        message.id.clone()
    }
}

/// A subscription message received by the Gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubSubscription {
    /// Action to perform.
    pub action: GossipsubSubscriptionAction,
    /// The topic from which to subscribe or unsubscribe.
    pub topic: TopicHash,
}

/// Action that a subscription wants to perform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GossipsubSubscriptionAction {
    /// The remote wants to subscribe to the given topic.
    Subscribe,
    /// The remote wants to unsubscribe from the given topic.
    Unsubscribe,
}

pub struct ControlMessage {
    ihave: Vec<ControlIHave>,
    iwant: Vec<ControlIWant>,
    graft: Vec<ControlGraft>,
    prune: Vec<ControlPrune>,
}

/// Gossip control message; this notifies the peer that the following
/// messages were recently seen and are available on request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlIHave {
    /// Topic that the messages belong to.
    pub topic: TopicHash,
    /// List of messages that have been recently seen and are available
    /// on request.
    pub messages: Vec<MessageID>,
}

/// Control message that requests messages from a peer that announced them
/// with an IHave message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlIWant {
    /// List of messages that are being requested.
    pub messages: Vec<MessageID>,
}

/// Control message that grafts a mesh link; this notifies the peer that it
/// has been added to the local mesh view of a topic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlGraft {
    /// Topic to graft a peer to.
    pub topic: TopicHash,
}

/// Control message that prunes a mesh link; this notifies the peer that it
/// has been removed from the local mesh view of a topic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ControlPrune {
    /// Topic to prune a peer from.
    pub topic: TopicHash,
}

/// A graft/prune received by the Gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubGraftPrune {
    /// Graft or prune action to perform.
    pub action: GossipSubGraftPruneAction,
    /// The topic from which to graft a peer to or prune from.
    pub topic: TopicHash,
}

/// Action to graft or prune to/from a topic. Manages mesh membership.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GossipSubGraftPruneAction {
    /// The remote wants to graft to the given topic.
    Graft(ControlGraft),
    /// The remote wants to prune from the given topic.
    Prune(ControlPrune),
}
