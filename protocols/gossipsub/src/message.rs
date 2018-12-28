use rpc_proto;

use libp2p_floodsub::TopicHash;

use libp2p_core::PeerId;

use bs58;
use chrono::{DateTime, Utc};
use protobuf::Message;

// May not be necessary.
// /// Contains an incrementing sequence number.
// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
// pub struct SeqNo {
//     seq_no: Vec<u8>,
// }

// impl SeqNo {

// }

/// A message received by the Gossipsub system.
/// > **Note**: message is unsized. FMI see
/// > https://github.com/libp2p/specs/issues/118.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GMessage {
    /// Id of the peer that published this message.
    pub source: PeerId,

    /// Content of the message. Its meaning is out of scope of this library.
    pub data: Vec<u8>,

    /// An incrementing sequence number.
    pub seq_no: Vec<u8>,

    /// List of topics this message belongs to.
    ///
    /// Each message can belong to multiple topics at once.
    pub topics: Vec<TopicHash>,

    // To use for an authentication scheme (not yet defined or implemented),
    // see rpc.proto for more info.
    // TODO
    // signature: Vec<u8>,

    // To use for an encryption scheme (not yet defined or implemented),
    // see rpc.proto for more info.
    // TODO
    // key: Vec<u8>,

    // TODO: there might be interoperability issues caused by these two fields.

    // This should not be public as it could then be manipulated. It needs to
    // only be modified via the `publish` method on `Gossipsub`. Used for the
    // message cache.
    time_sent: DateTime<Utc>,

    // The hash of the message.
    hash: MsgHash,
}

impl GMessage {
    // Sets the hash of the message, used in `MsgHashBuilder`.
    #[inline]
    pub(crate) fn set_hash(&mut self, msg_hash: MsgHash) -> &mut Self {
        self.hash = msg_hash;
        self
    }

    /// Returns the hash of the message.
    #[inline]
    pub fn get_hash(&self) -> &MsgHash {
        &self.hash
    }

    // As above, used in the `publish` method on `Gossipsub` for `MCache`.
    pub(crate) fn set_timestamp(&mut self) {
        self.time_sent = Utc::now();
    }
}

/// Contains a message ID as a string, has impls for building and converting to a `String`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MsgId {
    /// The message ID as a string.
    id: String,
}

impl MsgId {
    /// Builds a new `MsgId` from the `seq_no` and `source` of a `Message`.
    #[inline]
    pub fn from_raw(msg: GMessage) -> MsgId {
        let id = format!("{}{}", String::from_utf8(msg.seq_no)
            .expect("Found invalid UTF-8"), msg.source.to_base58());
        MsgId {
            id: id,
        }
    }

    /// Converts a `MsgId` into a message ID as a `String`.
    #[inline]
    pub fn into_string(self) -> String {
        self.id
    }
}

// impl From<GMessage> for MsgId {
//     #[inline]
//     fn from(message: GMessage) -> Self {
//         message.id
//     }
// }

/// Represents the hash of a `Message`.
///
/// Instead of a using the message as a whole, the API of floodsub may use a
/// hash of the message. You only have to build the hash once, then use it
/// everywhere.
///
/// > **Note** https://github.com/libp2p/specs/issues/116#issuecomment-450107520. "A potential caveat with using hashes instead of seqnos: the peer won't be able to send identical messages (e.g. keepalives) within the timecache interval, as they will get rejected as duplicates."
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MsgHash {
    hash: String,
}

impl MsgHash {
    /// Builds a new `MsgHash` from the given hash.
    #[inline]
    pub fn from_raw(hash: String) -> MsgHash {
        MsgHash { hash: hash }
    }

    /// Converts a `MsgHash` into a hash of the message as a `String`.
    #[inline]
    pub fn into_string(self) -> String {
        self.hash
    }
}

/// Builder for a `MsgHash`.
#[derive(Debug, Clone)]
pub struct MsgHashBuilder {
    builder: rpc_proto::Message,
}

impl MsgHashBuilder {
    // pub fn new(msg: Message) -> Self
    // {
    //     let mut builder = msg;

    //     MsgHashBuilder { builder: builder }
    // }

    /// Turns the builder into an actual `MsgHash`.
    pub fn build(self) -> MsgHash {
        let bytes = self
            .builder
            .write_to_bytes()
            .expect("protobuf message is always valid");
        MsgHash {
            hash: bs58::encode(&bytes).into_string(),
        }
    }
}

/// Contains either a `MsgHash` or a `MsgId`, to represent a
/// message.
pub enum MsgRepEnum {
    hash(MsgHash),
    id(MsgId),
}

// Not used? May delete.
/// Represents a message, which can be a hash of the message or a
/// message ID as a string. It is used to query for a message.
// This could actually be part of rpc.proto, saving the need to manually write
// this.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MsgRep {
    /// The hash of the `GMessage`.
    pub hash: MsgHash,
    /// The `GMessage` ID.
    pub id: MsgId,
}

impl MsgRep {
    #[inline]
    pub fn new() -> MsgRep {
        ::std::default::Default::default()
    }

    /// Convenience function that sets the `hash` field of `MsgRep`
    /// with the provided `MsgHash` instance.
    pub fn set_msg_hash(&mut self, hash: MsgHash) {
        self.hash = hash
    }

    /// Convenience function that sets the `id` field of `MsgRep` with
    /// the provided `MsgId` instance.
    pub fn set_msg_id(&mut self, id: MsgId) {
        self.id = id
    }
}

impl AsRef<MsgHash> for MsgRep {
    #[inline]
    fn as_ref(&self) -> &MsgHash {
        &self.hash
    }
}

impl From<MsgRep> for MsgHash {
    #[inline]
    fn from(message: GMessage) -> MsgHash {
        message.hash
    }
}

impl<'a> From<&'a MsgRep> for MsgHash {
    #[inline]
    fn from(message: &'a GMessage) -> MsgHash {
        message.hash.clone()
    }
}

impl AsRef<MsgId> for MsgRep {
    #[inline]
    fn as_ref(&self) -> &MsgId {
        &self.id
    }
}

impl From<MsgRep> for MsgId {
    #[inline]
    fn from(message: MsgRep) -> MsgId {
        message.id
    }
}

impl<'a> From<&'a MsgRep> for MsgId {
    #[inline]
    fn from(message: &'a MsgRep) -> MsgId {
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

/// Contains the control message for Gossipsub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ControlMessage {
    /// The control message for gossiping
    pub ihave: Vec<ControlIHave>,
    /// Request transmission of messages announced in a `ControlIHave` message.
    pub iwant: Vec<ControlIWant>,
    /// Graft a mesh link; this notifies the peer that it has been added to
    /// the local mesh view.
    pub graft: Vec<ControlGraft>,
    /// The control message for pruning mesh links.
    pub prune: Vec<ControlPrune>,
}

/// Gossip control message; this notifies the peer that the following
/// messages were recently seen and are available on request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ControlIHave {
    /// Topic that the messages belong to.
    pub topic: TopicHash,
    /// List of messages that have been recently seen and are available
    /// on request.
    pub messages: Vec<MsgRep>,
}

/// Control message that requests messages from a peer that announced them
/// with an IHave message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ControlIWant {
    /// List of messages that are being requested.
    pub messages: Vec<MsgRep>,
}

/// Control message that grafts a mesh link; this notifies the peer that it
/// has been added to the local mesh view of a topic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ControlGraft {
    /// Topic to graft a peer to.
    pub topic: TopicHash,
}

/// Control message that prunes a mesh link; this notifies the peer that it
/// has been removed from the local mesh view of a topic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
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

/// An RPC received by the Gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<GMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<GossipsubSubscription>,
    /// Optional control message.
    pub control: Option<ControlMessage>,
}
