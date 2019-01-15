use mcache::MCache;
use rpc_proto;

use {Topic, TopicMap, TopicHash};

use libp2p_core::PeerId;

use bs58;
// use chrono::{DateTime, Utc};
use protobuf::Message;
use std::{
    borrow::Borrow,
    collections::hash_map::{HashMap, Keys},
    hash::{Hash, Hasher},
    io,
    iter::{Iterator, IntoIterator},
};

/// Used in MCache.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MsgMap(HashMap<MsgHash, GMessage>);

impl MsgMap {
    pub fn new() -> Self {
        MsgMap(HashMap::new())
    }

    pub fn insert(&mut self, mh: MsgHash, m: GMessage) -> Option<GMessage> {
        self.0.insert(mh, m)
    }

    pub fn get<Q: ?Sized>(&self, k: &Q) -> Option<&GMessage>
    where
        MsgHash: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.0.get(k)
    }

    pub fn keys(&self) -> Keys<MsgHash, GMessage> {
        self.0.keys()
    }

    pub fn remove<Q: ?Sized>(&self, k: &Q) -> Option<GMessage>
    where
        MsgHash: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.0.remove(k)
    }
}

/// A message received by the Gossipsub system.
///
/// Recently seen messages are stored in `MCache`. They can be retrieved from
/// this message cache via a `gossipsub::TopicMap` by querying with a `MsgRep`,
/// which contains either
/// a `MsgHash` or `MsgId`, where the latter is more desirable for privacy.
/// This contains the same public fields as a
/// `floodsub::protocol::FloodsubMessage`.
/// The message is limited to 1 MiB, which is enforced by a check when
/// publishing the message.
// It seems that `Hash` needs to be implemented manually, as `TopicMap` is a
// `HashMap` tuple struct, and `Hash` shouldn't/can't be implemented/derived
// for a `HashMap`.
// The add method passes a `MsgHash` to a Gossipsub.received, and the compiler
// complains if `Hash` isn't implemented.
// Better to use accessors for the fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GMessage {
    /// ID of the peer that published this message.
    pub(crate) from: PeerId,

    /// Content of the message. Its meaning is out of scope of this library.
    pub(crate) data: Vec<u8>,

    /// An incrementing sequence number.
    pub(crate) seq_no: Vec<u8>,

    /// List of topics this message belongs to.
    ///
    /// Each message can belong to multiple topics at once.
    // Issue with using a HashMap with rust-protobuf:
    // https://github.com/stepancheg/rust-protobuf/issues/211
    // See also the note on the descriptor field of the Topic struct.
    pub(crate) topics: TopicMap,

    // To use for an authentication scheme (not yet defined or implemented),
    // see rpc.proto for more info.
    // TODO
    // signature: Vec<u8>,

    // To use for an encryption scheme (not yet defined or implemented),
    // see rpc.proto for more info.
    // TODO
    // key: Vec<u8>,

    // TODO: there might be interoperability issues caused by these two fields.
    // They may be moved to `MCache`.

    // Fields that are not in rpc_proto::RPC should not be included here
    // without including them there, otherwise there will be an unresolvable
    // compiler error for uninitialized fields when trying to decode it.
    // This should not be public as it could then be manipulated. It needs to
    // only be modified via the `publish` method on `Gossipsub`. Used for the
    // message cache.
    // Therefore further thought is needed for where to store the time_sent
    // and hash. The hash is not an issue since it can be converted to a topic
    // via construction and reverse construction. Experiment with storing both
    // in `MCache` or `Gossipsub`.
    // pub(crate) time_sent: DateTime<Utc>,

    // // The hash of the message.
    // pub(crate) hash: MsgHash,
}

impl Hash for GMessage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.from.hash(state);
        self.data.hash(state);
        self.seq_no.hash(state);
    }
}
impl GMessage {
    pub fn get_from(&self) -> PeerId {
        self.from
    }

    pub fn get_data(&self) -> Vec<u8> {
        self.data
    }

    pub fn get_seq_no(&self) -> Vec<u8> {
        self.seq_no
    }

    pub fn get_topic_map(&self) -> TopicMap {
        self.topics
    }


    // // Sets the hash of the message, used in `MsgHashBuilder`.
    // #[inline]
    // pub(crate) fn set_hash(&mut self, msg_hash: MsgHash) {
    //     self.hash = msg_hash;
    // }

    // /// Returns the hash of the message.
    // #[inline]
    // pub fn get_hash(&self) -> &MsgHash {
    //     &self.hash
    // }

    // // As above, used in the `publish` method on `Gossipsub` for `MCache`.
    // pub(crate) fn set_timestamp(&mut self) {
    //     self.time_sent = Utc::now();
    // }

    // /// Returns the timestamp of the message.
    // pub fn get_timestamp(&self) -> &DateTime<Utc> {
    //     &self.time_sent
    // }

    // // As above, used in the `publish` method on `Gossipsub` for `MCache`.
    // pub(crate) fn set_id(&mut self, msg_id: MsgId) {
    //     self.id = Some(msg_id);
    // }

    // /// Returns the id of the message, if it has been set.
    // pub fn get_id(&self) -> &Option<MsgId> {
    //     &self.id
    // }

    pub fn from_proto_msg(p_msg: rpc_proto::Message)
        -> Result<GMessage, io::Error> {
        let from = PeerId::from_bytes(p_msg.get_from().to_vec()).expect("The from field of rpc_proto is not a valid peer ID");
        let topic_hashes = p_msg.get_topic_hashes().to_vec().iter();
        let topic_map = TopicMap::new();
        for th_str in topic_hashes {
            let th = TopicHash::from_raw(th_str.to_string());
            let topic = Topic::from(th);
            topic_map.insert(th, topic);
        }

        Ok(GMessage {
            from: from,
            data: p_msg.get_data().to_vec(),
            seq_no: p_msg.get_seqno().to_vec(),
            topics: topic_map,
        })
    }

    pub fn from_msg_hash(m_hash: MsgHash) -> Result<GMessage, io::Error> {
        let decoded_hash: &[u8]
            = bs58::decode(m_hash.hash).into_vec().unwrap().as_ref();
        let rpc_msg = protobuf::parse_from_bytes::<rpc_proto::Message>(
            decoded_hash).unwrap();
        GMessage::from_proto_msg(rpc_msg)
    }
}

// This should be TryFrom, but this is nightly.
// impl From<MsgHash> for GMessage {
//     fn from(m_hash: MsgHash) -> Self {
//         let decoded_hash: &[u8]
//             = bs58::decode(m_hash.hash).into_vec().unwrap().as_ref();
//         let rpc_msg = protobuf::parse_from_bytes::<rpc_proto::Message>
//             (decoded_hash).unwrap();
//         GMessage::from(rpc_msg)
//     }
// }

// Incomplete: TryFrom is a nightly only experimental API.
// impl TryFrom<rpc_proto::Message> for GMessage {
//     type Error = TryFromMsgError;
//     fn try_from(p_msg: rpc_proto::Message)
//         -> Result<GMessage, TryFromMsgError> {
//         GMessage {
//             from: PeerId::from_bytes(p_msg.get_from()).map_err(|e| ),
//             data: p_msg.get_data(),
//             seq_no: p_msg.get_seqno(),
//             topics: p_msg.get_topic_hashes(),
//         }
//     }
// }

impl From<GMessage> for rpc_proto::Message {
    fn from(message: GMessage) -> rpc_proto::Message {
        let mut msg = rpc_proto::Message::new();
        msg.set_from(message.from.into_bytes());
        msg.set_data(message.data);
        msg.set_seqno(message.seq_no);

        let mut t_hashes = ::protobuf::RepeatedField::new();
        for t_hash in message.topics.keys() {
            t_hashes.push((*t_hash).into_string());
        }
        msg.set_topic_hashes(t_hashes);
        // msg.set_signature(message.signature);
        // msg.set_key(message.key);
        msg
    }
}

// impl From<GMessage> for MsgHash {
//     fn from(message: GMessage) -> MsgHash {
//     }
// }

// It seems that we can't actually impl this since a message might not contain
// a message ID.
// impl From<GMessage> for MsgId {
//     fn from(message: GMessage) -> MsgId {
//         let m_id = message.get_id();
//         if m_id.is_some() {
//             return m_id.expect("We checked m_id with `is_some`.")
//         } else {

//         }

//     }
// }

// See note below.
/// Contains a message ID as a string, has impls for building and converting
/// to a `String`.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MsgId {
    /// The message ID as a string.
    id: String,
}

impl MsgId {
    /// Builds a new `MsgId` from the `seq_no` and `source` of a `Message`.
    #[inline]
    pub fn new(msg: GMessage) -> MsgId {
        let id = format!("{}{}", String::from_utf8(msg.seq_no)
            .expect("Found invalid UTF-8"), msg.from.to_base58());
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

// Unlike a `MsgHash`, we can't rebuild a `GMessage` from a `MsgId`(or
// at least it isn't as easy).
// We have to fetch it from somewhere it is stored. In this context,
// this would be the `MCache`, although messages are only stored for a
// few seconds / heartbeat intervals, hence implementing `From` won't work.
// Using `TryFrom` also adds complications.
// An alternative is to reconstruct a `GMessage` from a `MsgId` by searching
// for a message that has the same seq_no and peer ID in all the state, but
// this is resource-intensive. Why do that when it seems simpler to just use
// a `MsgHash`? It is therefore recommended to do that: just use a MsgHash,
// and not use and probably remove a `MsgId` and `MsgRep`, which will also
// simplify implementation.
//     fn try_from(t_id: TopicId) -> Result<Self, Self::Error> {
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
/// > **Note**
/// > "A potential caveat with using hashes instead of seqnos: the peer won't
/// > be able to send identical messages (e.g. keepalives) within the
/// > timecache interval, as they will get rejected as duplicates."
/// —https://github.com/libp2p/specs/issues/116#issuecomment-450107520
/// > However, I think `MsgRep` enum may be a solution for this, and the
/// > `MsgHash` is constructed from the whole message, not just the `seq_no`
/// > and `source` fields.
// Since a `MsgId` is unique for every message, by concatenating the peer_id
// and seq_no fields of the message, it may suffice to simply hash the MsgId,
// rather than the whole message. However, by hashing the message we can
// reconstruct the message from the hash, which may be useful instead of
// searching for it within the `Gossipsub` state or `MCache`.
// If the message needs to be private then converting a MsgHash to a GMessage
// can be restricted from public use (including the result of the message).
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MsgHash {
    hash: String,
}

impl MsgHash {
    /// Builds a new `MsgHash` from the message that it represents.
    pub fn new(msg: GMessage) -> Self {
        MsgHashBuilder::new(msg).build()
    }

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
    pub fn new(msg: GMessage) -> Self {
        let builder = rpc_proto::Message::from(msg);

        MsgHashBuilder { builder: builder }
    }

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

// See note on MsgId above.
/// Contains either a `MsgHash` or a `MsgId` to represent a message.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MsgRep {
    hash(MsgHash),
    id(MsgId),
}

/// A subscription message received by the Gossipsub system.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GossipsubSubscription {
    /// Action to perform.
    pub action: GossipsubSubscriptionAction,
    /// The topic from which to subscribe or unsubscribe.
    pub topic: TopicHash,
}

impl From<GossipsubSubscription> for rpc_proto::RPC_SubOpts {
    fn from(gsub: GossipsubSubscription) -> rpc_proto::RPC_SubOpts {
        let mut subscription = rpc_proto::RPC_SubOpts::new();
        subscription.set_subscribe(gsub.action
            == GossipsubSubscriptionAction::Subscribe);
        subscription.set_topic_hash(gsub.topic.into_string());
        subscription
    }
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
///
/// Included in `GossipsubRpc`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlMessage {
    /// The control message for gossiping
    pub(crate) ihave: Vec<ControlIHave>,
    /// Request transmission of messages announced in a `ControlIHave` message.
    pub(crate) iwant: Vec<ControlIWant>,
    /// Graft a mesh link; this notifies the peer that it has been added to
    /// the local mesh view.
    pub(crate) graft: Vec<ControlGraft>,
    /// The control message for pruning mesh links.
    pub(crate) prune: Vec<ControlPrune>,
}

impl ControlMessage {
    fn new() -> Self {
        ControlMessage::default()
    }
}

impl From<ControlMessage> for rpc_proto::ControlMessage {
    fn from(control: ControlMessage) -> rpc_proto::ControlMessage {
        let mut ctrl = rpc_proto::ControlMessage::new();

        for control_i_have in control.ihave.into_iter() {
            let mut ctrl_i_have = rpc_proto::ControlIHave
                ::from(control_i_have);
            ctrl.get_ihave().to_vec().push(ctrl_i_have);
        }

        for control_i_want in control.iwant.into_iter() {
            let mut ctrl_i_want = rpc_proto::ControlIWant
                ::from(control_i_want);
            ctrl.get_iwant().to_vec().push(ctrl_i_want);
        }

        for control_graft in control.graft.into_iter() {
            let mut ctrl_graft = rpc_proto::ControlGraft
                ::from(control_graft);
            ctrl.get_graft().to_vec().push(ctrl_graft);
        }

        for control_prune in control.prune.into_iter() {
            let mut ctrl_prune = rpc_proto::ControlPrune
                ::from(control_prune);
            ctrl.get_prune().to_vec().push(ctrl_prune);
        }
        ctrl
    }
}

impl From<rpc_proto::ControlMessage> for ControlMessage {
    fn from(ctrl: rpc_proto::ControlMessage) -> ControlMessage {
        let mut control = ControlMessage::default();

        for ctrl_i_have in ctrl.get_ihave().into_iter() {
            let mut control_i_have = ControlIHave::from(*ctrl_i_have);
            control.ihave.push(control_i_have)
        }

        control
    }
}

/// Gossip control message; this notifies the peer that the following
/// messages were recently seen and are available on request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlIHave {
    /// Topic that the messages belong to, represented by `TopicHash`.
    pub t_hash: TopicHash,
    /// List of messages that have been recently seen and are available
    /// on request.
    pub recent_mcache: MCache,
}

impl ControlIHave {
    fn new() -> Self {
        ControlIHave {
            t_hash: TopicHash::new(),
            recent_mcache: MCache::new(),
        }
    }
}
impl From<ControlIHave> for rpc_proto::ControlIHave {
    fn from(control_i_have: ControlIHave) -> rpc_proto::ControlIHave {
        let mut ctrl_i_have = rpc_proto::ControlIHave::new();
        ctrl_i_have.set_topic_hash(control_i_have.t_hash.into_string());
        // For getting my head around this with seeing the return
        // types by hovering over, uncomment if you need to
        // do the same.
        // let bar_into_iter = control_i_have.recent_mcache.into_iter();
        // let map_bar_into_iter = bar_into_iter.map(|m| m.id.into_string());
        // let collect_map_bar_into_iter = map_bar_into_iter.collect();
        ctrl_i_have.set_message_hashes(control_i_have.recent_mcache.msgs_keys()
            .map(|m| m.into_string()).collect());
        ctrl_i_have
    }
}

impl From<rpc_proto::ControlIHave> for ControlIHave {
    fn from(ctrl_i_have: rpc_proto::ControlIHave) -> ControlIHave {
        let mut control_i_have = ControlIHave::default();
        control_i_have.t_hash = TopicHash::from_raw(ctrl_i_have
            .get_topic_hash().to_string());
        control_i_have.recent_mcache.put_many
            (ctrl_i_have.get_message_hashes().into_iter()
                .map(|mh| GMessage::from_msg_hash(
                    MsgHash::from_raw(mh.to_string()))
                    .expect("Error converting `MsgHash` to `GMessage`"))
                .collect::<Vec<_>>());
        control_i_have
    }
}

/// Control message that requests messages from a peer that announced them
/// with an IHave message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ControlIWant {
    /// A vector of message hashes, which are used to get messages.
    pub m_hashes: Vec<MsgHash>,
}

impl ControlIWant {
    fn new() -> Self {
        ControlIWant { m_hashes: vec!(MsgHash::default())}
    }
}

impl From<ControlIWant> for rpc_proto::ControlIWant {
    fn from(control_i_want: ControlIWant) -> rpc_proto::ControlIWant {
        let mut ctrl_i_want = rpc_proto::ControlIWant::new();
        ctrl_i_want.set_message_hashes(control_i_want.m_hashes.into_iter()
            .map(|mh| mh.into_string()).collect());
        ctrl_i_want
    }
}

/// Control message that grafts a mesh link; this notifies the peer that it
/// has been added to the local mesh view of a topic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ControlGraft {
    /// Topic to graft a peer to.
    pub topic: TopicHash,
}

impl From<ControlGraft> for rpc_proto::ControlGraft {
    fn from(control_graft: ControlGraft) -> rpc_proto::ControlGraft {
        let mut ctrl_graft = rpc_proto::ControlGraft::new();
        ctrl_graft.set_topic_hash(control_graft.topic.into_string());
        ctrl_graft
    }
}

/// Control message that prunes a mesh link; this notifies the peer that it
/// has been removed from the local mesh view of a topic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ControlPrune {
    /// Topic to prune a peer from.
    pub topic: TopicHash,
}

impl From<ControlPrune> for rpc_proto::ControlPrune {
    fn from(control_prune: ControlPrune) -> rpc_proto::ControlPrune {
        let mut ctrl_prune = rpc_proto::ControlPrune::new();
        ctrl_prune.set_topic_hash(control_prune.topic.into_string());
        ctrl_prune
    }
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
/// Included e.g. in the events field of `Gossipsub`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GossipsubRpc {
    /// List of messages that were part of this RPC query.
    pub messages: Vec<GMessage>,
    /// List of subscriptions.
    pub subscriptions: Vec<GossipsubSubscription>,
    /// Optional control message.
    pub control: Option<ControlMessage>,
}

/// Contains the different types of events that we yield to the outside when
/// polling, which are `GMessage` and `message::ControlMessage`. Used in
/// the events field of `layer::Gossipsub`  as the `TOutEvent` for the
/// `NetworkBehaviourAction`.
pub enum GOutEvents {
    /// The `GMessage` to send.
    GMsg(GMessage),
    /// The `ControlMessage` to send.
    CtrlMsg(ControlMessage),
}
