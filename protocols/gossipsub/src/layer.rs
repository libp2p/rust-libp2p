// use cuckoofilter::CuckooFilter;
use libp2p_floodsub::Floodsub;
use libp2p_core::swarm::{NetworkBehaviour, NetworkBehaviourAction};
// use libp2p_core::{
//     swarm::{NetworkBehaviour, NetworkBehaviourAction},
//     PeerId,
// };
use protocol::{GossipsubRpc, GossipsubMessage, ControlMessage};
// use smallvec::SmallVec;
// use std::collections::hash_map::{DefaultHasher, HashMap};
// use smallvec::SmallVec;
use std::collections::VecDeque;
// use std::{collections::VecDeque, marker::PhantomData};
// use topic::{Topic, TopicHash};

pub struct Gossibsub<TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, GossipsubMessage,
        ControlMessage>>,

    floodsub: Floodsub,
    // Don't think these are needed to be duplicated since they are in the
    // above field.
    // /// Peer id of the local node. Used for the source of the messages that we
    // /// publish.
    // local_peer_id: PeerId,

    // /// List of peers the network is connected to, and the topics that they're
    // /// subscribed to.
    // // TODO: filter out peers that don't support floodsub, so that we avoid
    // //       hammering them with opened substreams
    // connected_peers: HashMap<PeerId, SmallVec<[TopicHash; 8]>>,

    // // List of topics we're subscribed to. Necessary to filter out messages
    // // that we receive erroneously.
    // subscribed_topics: SmallVec<[Topic; 16]>,

    // // We keep track of the messages we received (in the format `hash(source
    // // ID, seq_no)`) so that we don't dispatch the same message twice if we
    // // receive it twice on the network.
    // received: CuckooFilter<DefaultHasher>,

    // /// Marker to pin the generics.
    // marker: PhantomData<TSubstream>,
}

impl<TSubstream> Gossibsub<TSubstream> {
    /// Creates a `Gossipsub`.
    pub fn new(local_peer_id: PeerId) -> Self {
        Gossibsub {
            events: VecDeque::new(),
            Floodsub::new(local_peer_id),
        }
    }

    /// Grafts the peer to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were
    /// already subscribed.
    pub fn graft(topic: TopicHash) -> bool {

    }

    /// Prunes the peer from a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were
    /// already subscribed.
    pub fn prune(topic: TopicHash) -> bool {

    }

    /// gossip; this notifies the peer that the following messages were
    /// recently seen and are available on request. Checks the seen set and
    /// requests unknown messages with an IWANT message.
    pub fn ihave(topics: Vec<TopicHash>) {

    }

    /// Request transmission of messages announced in an IHAVE message.
    /// Forwards all request messages that are present in mcache to the
    /// requesting peer.
    pub fn iwant(topics: Vec<TopicHash>) {

    }
}

impl NetworkBehaviour for Gossibsub<TSubstream> {

}