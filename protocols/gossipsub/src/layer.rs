use cuckoofilter::CuckooFilter;
use libp2p_floodsub::{Floodsub, Topic, TopicHash};
use libp2p_core::{
    swarm::{NetworkBehaviour, NetworkBehaviourAction},
    PeerId,
};
// use protocol::{GossipsubRpc, GossipsubMessage, ControlMessage};
use smallvec::SmallVec;
use std::collections::hash_map::{DefaultHasher, HashMap};
use smallvec::SmallVec;
use std::{collections::VecDeque, marker::PhantomData};

/// Contains the state needed to maintain the Gossipsub protocol.
///
/// We need to duplicate the same fields as `Floodsub` in order to
/// differentiate the state of the two protocols.
pub struct Gossibsub<TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, Message>>,

    /// Peer id of the local node. Used for the source of the messages that we
    /// publish.
    local_peer_id: PeerId,

    /// List of peers the network is connected to, and the topics that they're
    /// subscribed to.
    // TODO: filter out peers that don't support gossipsub, so that we avoid
    //       hammering them with opened substreams
    connected_peers: HashMap<PeerId, SmallVec<[TopicHash; 8]>>,

    // List of topics we're subscribed to. Necessary to filter out messages
    // that we receive erroneously.
    subscribed_topics: SmallVec<[Topic; 16]>,

    // We keep track of the messages we received (in the format `hash(source
    // ID, seq_no)`) so that we don't dispatch the same message twice if we
    // receive it twice on the network.
    received: CuckooFilter<DefaultHasher>,

    /// See `Mesh` for details.
    mesh: Mesh,

    /// The `Mesh` peers to which we are publishing to without topic
    /// membership, as a map of topics to lists of peers.
    fanout: Mesh,

    mcache: Vec<Message>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,

    /// For backwards-compatibility.
    floodsub: Floodsub<TSubstream>,
}

impl<TSubstream> Gossibsub<TSubstream> {
    /// Creates a `Gossipsub`.
    pub fn new(local_peer_id: PeerId) -> Self {
        Gossibsub {
            events: VecDeque::new(),
            Floodsub::new(local_peer_id),
        }
    }

    /// Grafts the peer to a topic. This notifies the peer that it has been /// added to the local mesh view.
    ///
    /// Returns true if the graft succeeded. Returns false if we were
    /// already grafted.
    pub fn graft(&mut self, topic: impl AsRef<TopicHash>) -> bool {

    }

    /// Grafts a peer to multiple topics.
    ///
    /// > **Note**: Doesn't do anything if we're already grafted to such
    /// > topics.
    pub fn graft_many<'a, I>(&self, topics: impl IntoIterator<Item = impl AsRef<TopicHash>>)
    {
        
    }

    /// Prunes the peer from a topic.
    ///
    /// Returns true if the peer is grafted to this topic.
    pub fn prune(&mut self, topic: impl AsRef<TopicHash>) {

    }

    /// Prunes the peer from multiple topics.
    ///
    /// Note that this only works if the peer is grafted to such topics.
    pub fn prune_many<'a, I>(&self, topics: impl IntoIterator<Item = impl AsRef<TopicHash>>)
    {

    }

    /// gossip; this notifies the peer that the following messages were
    /// recently seen and are available on request. Checks the seen set and
    /// requests unknown messages with an IWANT message.
    pub fn ihave(&mut self, topics: impl AsRef<TopicHash>) {

    }

    /// Request transmission of messages announced in an IHAVE message.
    /// Forwards all request messages that are present in mcache to the
    /// requesting peer.
    pub fn iwant(&mut self, topics: impl AsRef<TopicHash>) {

    }

    pub fn join(&mut self, topic: impl AsRef<TopicHash>) {

    }

    pub fn join_many(&self, topics: impl IntoIterator<Item = impl AsRef<TopicHash>>) {

    }

    pub fn leave(&mut self, topic: impl AsRef<TopicHash>) {

    }

    pub fn leave_many(&self, topics: impl IntoIterator<Item = impl AsRef<TopicHash>>) {

    }
}

impl NetworkBehaviour for Gossibsub<TSubstream> {

}

/// A soft overlay network for topics of interest, which meshes as a map
/// of topics to lists of peers. It is a randomized topic mesh as a map of a
/// topic to a list of peers. The local peer maintains a topic view of its
/// direct peers only, a subset of the total peers that subscribe to a topic,
/// in order to limit bandwidth and increase decentralization, security and
/// sustainability. Extracts from the [spec]
/// (https://github.com/libp2p/specs/tree/master/pubsub/gossipsub),
/// although FMI read the full spec:
/// > The overlay is maintained by exchanging subscription control messages
/// > whenever there is a change in the topic list. The subscription
/// > messages are not propagated further, so each peer maintains a topic
/// > view of its direct peers only. Whenever a peer disconnects, it is
/// > removed from the overlay…
/// > We can form an overlay mesh where each peer forwards to a subset of
/// > its peers on a stable basis… Each peer maintains its own view of the
/// > mesh for each topic, which is a list of bidirectional links to other
/// > peers. That is, in steady state, whenever a peer A is in the mesh of
/// > peer B, then peer B is also in the mesh of peer A.
///
/// > **Note**: as discussed in the spec, ambient peer discovery is pushed
/// > outside the scope of the protocol.
pub type Mesh = HashMap<TopicHash, Vec<PeerId>>;
