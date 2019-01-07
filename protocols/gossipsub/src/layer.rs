use handler::GossipsubHandler;
use mcache::MCache;
use mesh::Mesh;
use message::{GossipsubRpc, GMessage, ControlMessage, GossipsubSubscription,
    GossipsubSubscriptionAction, MsgHash, MsgId};
use {Topic, TopicHash, TopicRep};
use rpc_proto;

use libp2p_floodsub::{Floodsub, handler::FloodsubHandler};
use libp2p_core::{
    PeerId,
    protocols_handler::{ProtocolsHandler, ProtocolsHandlerSelect},
    swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction,
        PollParameters},
};

use chrono::Utc;
use cuckoofilter::CuckooFilter;
use futures::prelude::*;
use protobuf::Message;
use smallvec::SmallVec;
use std::{
    collections::{
        hash_map::{DefaultHasher, HashMap},
        VecDeque
    },
    iter,
    marker::PhantomData
};
use tokio_io::{AsyncRead, AsyncWrite};

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
    connected_peers: HashMap<PeerId, SmallVec<[TopicRep; 8]>>,

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
    pub fn new_w_existing_floodsub(local_peer_id: PeerId, fs: Floodsub<TSubstream>)
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
                    control: ::std::default::Default::default(),
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
                    control: ::std::default::Default::default(),
                },
            });
        }

        true
    }

    /// Publishes a message to the network, optionally choosing to set
    /// a message ID.
    ///
    /// > **Note**: Doesn't do anything if we're not subscribed to the topic.
    pub fn publish(&mut self, topic: impl Into<Topic>,
        data: impl Into<Vec<u8>>,
        control: Option<ControlMessage>,
        msg_id: bool) {
        self.publish_many(iter::once(topic), data, control, msg_id)
    }

    /// Publishes a message with multiple topics to the network, without any
    /// authentication or encryption.
    ///
    /// Optionally add a message ID.
    ///
    /// > **Note**: Doesn't do anything if we're not subscribed to any of the
    /// topics.
    pub fn publish_many(&mut self,
        topics: impl IntoIterator<Item = impl Into<Topic>>,
        data: impl Into<Vec<u8>>,
        control: Option<ControlMessage>,
        message_id: bool) {

        let message = GMessage {
            source: self.local_peer_id.clone(),
            data: data.into(),
            // If the sequence numbers are predictable, then an attacker could
            // flood the network with packets with the predetermined sequence
            // numbers and absorb our legitimate messages. We therefore use
            // a random number.
            seq_no: rand::random::<[u8; 20]>().to_vec(),
            topics: topics.into_iter().map(|t| t.into().clone()).collect(),
            time_sent: Utc::now(),
            hash: ::std::default::Default::default(),
            id: ::std::default::Default::default(),
        };

        if message_id {
            let m_id = MsgId::new(message);
            message.id = Some(m_id);
        }

        let msg_hash = MsgHash::new(message);

        message.set_hash(msg_hash);

        let proto_msg = rpc_proto::Message::from(message);
        // Check that the message size is less than or equal to 1 MiB.
        // TODO: test
        assert!(proto_msg.compute_size() <= 1048576);

        // Don't publish the message if we're not subscribed ourselves to any
        // of the topics.
        if !self.subscribed_topics.iter().any(|t| message.topics.values()
            .any(|u| t == u)) {
            return;
        }

        self.received.add(&message);

        // TODO: Send to peers that are in our message link and that we know
        // are subscribed to the topic.
        for (peer_id, sub_topic) in self.connected_peers.iter() {
            if !sub_topic.iter().any(|t| message.topics.values()
                .any(|u| Topic::from(t) == u)){
                continue;
            }

            message.set_timestamp();

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
    // pub fn graft(&mut self, topic: impl AsRef<TopicHash>) -> bool {
        
    // }

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
    type ProtocolsHandler =
        ProtocolsHandlerSelect<GossipsubHandler<TSubstream>,
        FloodsubHandler<TSubstream>>;
    type OutEvent = GMessage;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        GossipsubHandler::new().select(FloodsubHandler::new())
    }

    fn inject_connected(&mut self, id: PeerId, _: ConnectedPoint) {
        // We need to send our subscriptions to the newly-connected node.
        for topic in self.subscribed_topics.iter() {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic.hash().clone(),
                        action: GossipsubSubscriptionAction::Subscribe,
                    }],
                    control: None
                },
            });
        }

        self.connected_peers.insert(id.clone(), SmallVec::new());
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        let was_in = self.connected_peers.remove(id);
        debug_assert!(was_in.is_some());
    }

    fn inject_node_event(
        &mut self,
        propagation_source: PeerId,
        event: GossipsubRpc,
    ) {
        // Update connected peers topics
        for subscription in event.subscriptions {
            let mut remote_peer_topics = self.connected_peers
                .get_mut(&propagation_source)
                .expect("connected_peers is kept in sync with the peers we \
                are connected to; we are guaranteed to only receive events \
                from connected peers; QED");
            match subscription.action {
                GossipsubSubscriptionAction::Subscribe => {
                    if !remote_peer_topics.contains(&subscription.topic) {
                        remote_peer_topics.push(subscription.topic);
                    }
                }
                GossipsubSubscriptionAction::Unsubscribe => {
                    if let Some(pos) = remote_peer_topics.iter().position(|t| t == &subscription.topic ) {
                        remote_peer_topics.remove(pos);
                    }
                }
            }
        }

        // List of messages we're going to propagate on the network.
        let mut rpcs_to_dispatch: Vec<(PeerId, GossipsubRpc)> = Vec::new();

        for message in event.messages {
            // Use `self.received` to skip the messages that we have already received in the past.
            // Note that this can false positive.
            if !self.received.test_and_add(&message) {
                continue;
            }

            // Add the message to be dispatched to the user.
            if self.subscribed_topics.iter().any(|t| message.topics.iter().any(|u| t.hash() == u)) {
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(message.clone()));
            }

            // Propagate the message to everyone else who is subscribed to any of the topics.
            for (peer_id, subscr_topics) in self.connected_peers.iter() {
                if peer_id == &propagation_source {
                    continue;
                }

                if !subscr_topics.iter().any(|t| message.topics.iter().any(|u| t == u)) {
                    continue;
                }

                if let Some(pos) = rpcs_to_dispatch.iter().position(|(p, _)| p == peer_id) {
                    rpcs_to_dispatch[pos].1.messages.push(message.clone());
                } else {
                    rpcs_to_dispatch.push((peer_id.clone(), GossipsubRpc {
                        subscriptions: Vec::new(),
                        messages: vec![message.clone()],
                    }));
                }
            }
        }

        for (peer_id, rpc) in rpcs_to_dispatch {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id,
                event: rpc,
            });
        }
    }

    fn poll(
        &mut self,
        _: &mut PollParameters<TTopology>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Async::Ready(event);
        }

        Async::NotReady
    }
}
