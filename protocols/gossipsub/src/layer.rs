use cuckoofilter::CuckooFilter;
use libp2p_floodsub::{Floodsub, Topic, TopicHash};
use libp2p_core::{
    swarm::{NetworkBehaviour, NetworkBehaviourAction},
    PeerId
};
use mcache::MCache;
use mesh::Mesh;
use message::{GossipsubRpc, GMessage, ControlMessage, GossipsubSubscription,
    GossipsubSubscriptionAction};
use smallvec::SmallVec;
use std::{
    collections::{
        hash_map::{DefaultHasher, HashMap},
        VecDeque
    },
    iter,
    marker::PhantomData
};
/// Contains the state needed to maintain the Gossipsub protocol.
///
/// We need to duplicate the same fields as `Floodsub` in order to
/// differentiate the state of the two protocols.
pub struct Gossipsub<TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, GMessage>>,

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

    mcache: MCache,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,

    /// For backwards-compatibility.
    floodsub: Floodsub<TSubstream>,
}

impl<TSubstream> Gossipsub<TSubstream> {
    /// Creates a `Gossipsub`.
    pub fn new(local_peer_id: PeerId) -> Self {
        Gossipsub {
            events: VecDeque::new(),
            local_peer_id,
            connected_peers: HashMap::new(),
            subscribed_topics: SmallVec::new(),
            received: CuckooFilter::new(),
            mesh: Mesh::new(),
            fanout: Mesh::new(),
            mcache: MCache::new(),
            marker: PhantomData,
            floodsub: Floodsub::new(local_peer_id),
        }
    }

    /// Convenience function that creates a `Gossipsub` with/using a previously existing `Floodsub`.
    pub fn new_w_existing_floodsub(local_peer_id: PeerId, fs: Floodsub)
    -> Self {
        let mut gs = Gossipsub::new(local_peer_id);
        gs.floodsub = fs;
        gs
    }
    // ---------------------------------------------------------------------
    // The following section is re-implemented from
    // Floodsub. This is needed to differentiate state.
    // TODO: write code to reduce re-implementation like this, where most code
    // is unmodified, except for types being renamed from `Floodsub*` to
    // `*Gossipsub`.

    /// Subscribes to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were
    /// already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        if self.subscribed_topics.iter().any(|t| t.hash() == topic.hash()) {
            return false;
        }

        for peer in self.connected_peers.keys() {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic.hash().clone(),
                        action: GossipsubSubscriptionAction::Subscribe,
                    }],
                },
            });
        }

        self.subscribed_topics.push(topic);
        true
    }

    /// Unsubscribes from a topic.
    ///
    /// Note that this only requires a `TopicHash` and not a full `Topic`.
    ///
    /// Returns true if we were subscribed to this topic.
    pub fn unsubscribe(&mut self, topic: impl AsRef<TopicHash>) -> bool {
        let topic = topic.as_ref();
        let pos = match self.subscribed_topics.iter()
            .position(|t| t.hash() == topic) {
            Some(pos) => pos,
            None => return false
        };

        self.subscribed_topics.remove(pos);

        for peer in self.connected_peers.keys() {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic.clone(),
                        action: GossipsubSubscriptionAction::Unsubscribe,
                    }],
                },
            });
        }

        true
    }

    /// Publishes a message to the network.
    ///
    /// > **Note**: Doesn't do anything if we're not subscribed to the topic.
    pub fn publish(&mut self, topic: impl Into<TopicHash>,
        data: impl Into<Vec<u8>>) {
        self.publish_many(iter::once(topic), data)
    }

    /// Publishes a message with multiple topics to the network, without any
    /// authentication or encryption.
    ///
    /// > **Note**: Doesn't do anything if we're not subscribed to any of the
    /// topics.
    pub fn publish_many(&mut self,
        topic: impl IntoIterator<Item = impl Into<TopicHash>>,
        data: impl Into<Vec<u8>>,
        control: Option<ControlMessage>) {
        let message = GMessage {
            source: self.local_peer_id.clone(),
            data: data.into(),
            // If the sequence numbers are predictable, then an attacker could
            // flood the network with packets with the predetermined sequence
            // numbers and absorb our legitimate messages. We therefore use
            // a random number.
            sequence_number: rand::random::<[u8; 20]>().to_vec(),
            topics: topic.into_iter().map(|t| t.into().clone()).collect(),
            timestamp: self.set_timestamp()
        };

        // Don't publish the message if we're not subscribed ourselves to any of the topics.
        if !self.subscribed_topics.iter().any(|t| message.topics.iter().any(|u| t.hash() == u)) {
            return;
        }

        self.received.add(&message);

        // Send to peers we know are subscribed to the topic.
        for (peer_id, sub_topic) in self.connected_peers.iter() {
            if !sub_topic.iter().any(|t| message.topics.iter().any(|u| t == u)) {
                continue;
            }

            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: vec![message.clone()],
                    control: control
                }
            });
        }
    }
    // End of re-implementation from `Floodsub` methods.
    // ---------------------------------------------------------------------

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

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for
    Gossipsub<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{

}
