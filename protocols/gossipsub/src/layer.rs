use errors::{GError, Result as GResult};
use handler::GossipsubHandler;
use mcache::MCache;
use mesh::Mesh;
use message::{ControlIHave, ControlIWant, ControlMessage, GMessage,
    GossipsubRpc, GossipsubSubscription, GossipsubSubscriptionAction,
    GOutEvents, MsgHash, MsgMap};
use {Topic, TopicHash};
use rpc_proto;

use libp2p_floodsub::Floodsub;
use libp2p_core::{
    PeerId,
    protocols_handler::ProtocolsHandler,
    swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction,
        PollParameters},
};
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
// Doesn't derive `Debug` because `CuckooFilter` doesn't.
pub struct Gossipsub<'a, TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    /// `TInEvent = GossipsubRpc`, `TOutEvent = GOutEvents` (the latter is
    /// either a `GMessage` or a `ControlMessage`).
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, GOutEvents>>,

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

    last_pub: HashMap<TopicHash, i64>,

    /// Pending gossips.
    gossip: HashMap<PeerId, Vec<&'a ControlIHave>>,

    /// Pending control messages.
    control: HashMap<PeerId, Vec<&'a ControlMessage>>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,

    /// For backwards-compatibility.
    // May not work, test.
    floodsub: Option<Floodsub<TSubstream>>,
}

impl<'a, TSubstream> Gossipsub<'a, TSubstream> {
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
            last_pub: HashMap::new(),
            gossip: HashMap::new(),
            control: HashMap::new(),
            marker: PhantomData,
            floodsub: None,
        }
    }

    /// Convenience function that creates a `Gossipsub` with/using a previously existing `Floodsub`.
    pub fn new_w_existing_floodsub(local_peer_id: PeerId,
        fs: Floodsub<TSubstream>) -> Self {
        let mut gs = Gossipsub::new(local_peer_id);
        gs.floodsub = Some(fs);
        gs
    }

    // ---------------------------------------------------------------------
    // The following section is re-implemented from
    // Floodsub. This is needed to differentiate state.
    // TODO: write code to reduce re-implementation like this, where most code
    // is unmodified, except for types being renamed from `Floodsub*` to
    // `Gossipsub*`.

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
    /// > **Note**: Doesn't do anything if we're not subscribed to any of the
    /// topics.
    //
    // Optionally add a message ID.
    pub fn publish_many(&mut self,
        topics: impl IntoIterator<Item = impl Into<Topic>>,
        data: impl Into<Vec<u8>>,
        control: Option<ControlMessage>,
        _message_id: bool) {

        let message = GMessage {
            from: self.local_peer_id.clone(),
            data: data.into(),
            // If the sequence numbers are predictable, then an attacker could
            // flood the network with packets with the predetermined sequence
            // numbers and absorb our legitimate messages. We therefore use
            // a random number.
            seq_no: rand::random::<[u8; 20]>().to_vec(),
            topics: topics.into_iter().map(|t| t.into().clone()).collect(),
            // time_sent: Utc::now(),
            // hash: ::std::default::Default::default(),
            // id: ::std::default::Default::default(),
        };

        // if message_id {
        //     let m_id = MsgId::new(message);
        //     message.id = Some(m_id);
        // }

        self.mcache.put(message.clone());

        let proto_msg = rpc_proto::Message::from(&message);
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
                .any(|u| Topic::from(t) == *u)){
                continue;
            }

            // message.set_timestamp();

            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: vec![message.clone()],
                    control: control.clone()
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
    pub fn graft(&mut self, t_hash: impl AsRef<TopicHash>)
        -> GResult<()> {
        self.graft_many(iter::once(t_hash))
    }

    // TODO: finish writing these methods
    // TODO: avoid excessive cloning.

    /// Grafts a peer to multiple topics, if they are subscribed to all of
    /// them. If the topic is not in the mesh it adds it and grafts the peer.
    pub fn graft_many(&mut self, t_hashes: impl IntoIterator<Item = impl
        AsRef<TopicHash>>) -> GResult<()> {
        let m = &mut self.mesh;
        for t_hash in t_hashes {
            let thr = t_hash.as_ref();
            let th_str = thr.clone().into_string();
            let peer = self.local_peer_id.clone();
            let peer_str = peer.to_base58();
            if !self.subscribed_topics.iter().any(|t| t.hash() == thr) {
                return Err(GError::NotSubscribedToTopic{t_hash: th_str,
                peer_id: peer_str, err: "Tried to graft the peer \
                '{peer_str}' to the topic with topic hash '{th_str}'."
                .to_string()});
            }

            match m.remove(thr) {
                Ok(mut ps) => {
                    match m.get_peer_from_topic(thr, &peer) {
                        Ok(_peer) => return Err(GError::AlreadyGrafted{
                            t_hash: th_str, peer_id: peer_str,
                            err: "".to_string()}),
                        Err(GError::NotGraftedToTopic{t_hash: _th_str,
                            peer_id: _peer_str, err: _err}) => {
                                ps.push(peer);
                                m.insert(thr.clone(), ps);
                            },
                        Err(GError::TopicNotInMesh{t_hash: _t_hash, err: _err})
                        => {m.insert(thr.clone(), vec!(peer));},
                        Err(err) => {return Err(err);} // Shouldn't happen.
                    }
                },
                Err(err) => {return Err(err);} // Shouldn't happen.
            }
        }
        Ok(())
    }

    /// Prunes the peer from a topic.
    ///
    /// Returns true if the peer is grafted to this topic.
    pub fn prune(&mut self, t_hash: impl AsRef<TopicHash>) -> GResult<()> {
        self.prune_many(iter::once(t_hash))
    }

    /// Prunes the peer from multiple topics.
    ///
    /// Note that this only works if the peer is grafted to such topics.
    pub fn prune_many(&mut self, t_hashes: impl IntoIterator<Item = impl
        AsRef<TopicHash>>) -> GResult<()> {
        let p = &self.local_peer_id;
        let m = &mut self.mesh;
        for t_hash in t_hashes {
            let thr = t_hash.as_ref();
            let th = thr.clone();
            let _th_str = th.into_string();
            let _p_str = p.clone().to_base58();

            match m.get_peer_from_topic(thr, p) {
                Ok(_peer) => {
                    m.remove_peer_from_topic(thr, p);
                },
                Err(GError::NotGraftedToTopic{t_hash: th_str, peer_id: p_str,
                err: _err}) => {
                    return Err(GError::NotGraftedToTopic{t_hash: th_str,
                    peer_id: p_str,
                    err: "{get_err} Tried to prune the peer '{p_str}' to the \
                    topic with topic hash '{th_str}'.".to_string()});
                },
                Err(GError::TopicNotInMesh{t_hash, err: _err})
                => {return Err(GError::TopicNotInMesh{t_hash,
                    err: "{err} Tried to prune the peer '{p_str}' to the \
                    topic with topic hash '{th_str}'.".to_string()});},
                Err(err) => {return Err(err);} // Shouldn't happen.
            }
        }
        Ok(())
    }

    /// gossip: this notifies the peer that the input messages
    /// were recently seen and are available on request.
    /// Checks the seen set and requests unknown messages with an IWANT
    /// message.
    pub fn i_have(&mut self,
        msg_hashes: impl IntoIterator<Item = impl AsRef<MsgHash>>) {
        let i_want = ControlIWant::new();
    }

    /// Requests transmission of messages announced in an IHAVE message.
    /// Forwards all request messages that are present in mcache to the
    /// requesting peer, as well as (unlike the spec and Go impl) returning
    /// those that are not, via reconstructing them from the message hashes.
    pub fn i_want(&mut self,
        msg_hashes: impl IntoIterator<Item = impl AsRef<MsgHash>>)
            -> GResult<(MsgMap, MsgMap)> {
        let mut return_msgs = MsgMap::new();
        let mut not_found = MsgMap::new();
        for msg_hash in msg_hashes {
            let mhr = msg_hash.as_ref();
            // let mh = |mhr: &MsgHash| {mhr.clone()};
            if let Some(msg) = self.mcache.get(mhr.clone()) {
                return_msgs.insert(mhr.clone(), msg.clone());
            } else {
                match GMessage::from_msg_hash(mhr.clone()) {
                    Ok(m) => {not_found.insert(mhr.clone(), m);},
                    Err(err) => {return Err(err);}
                }
            }
        }
        Ok((return_msgs, not_found))
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

impl<'a, TSubstream, TTopology> NetworkBehaviour<TTopology> for
    Gossipsub<'a, TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = GossipsubHandler<TSubstream>;
    // TODO: this seems rather complicated to implement instead of the above
    // e.g. with inject_node_event—the event input, etc.
    // type ProtocolsHandler =
    //     ProtocolsHandlerSelect<GossipsubHandler<TSubstream>,
    //     FloodsubHandler<TSubstream>>;
    type OutEvent = GOutEvents;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        GossipsubHandler::new()
        //.select(FloodsubHandler::new())
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
                    if let Some(pos) = remote_peer_topics.iter()
                        .position(|t| t == &subscription.topic) {
                        remote_peer_topics.remove(pos);
                    }
                }
            }
        }

        // List of messages we're going to propagate on the network.
        let mut rpcs_to_dispatch: Vec<(PeerId, GossipsubRpc)> = Vec::new();

        for message in event.messages {
            // Use `self.received` to skip the messages that we have already
            // received in the past.
            // Note that this can be a false positive.
            if !self.received.test_and_add(&message) {
                continue;
            }

            // Add the message to be dispatched to the user, if they're
            // subscribed to the topic.
            if self.subscribed_topics.iter().any(|t| message.topics.iter()
                .any(|u| t == u.1)) {
                self.events.push_back(NetworkBehaviourAction::GenerateEvent
                    (GOutEvents::GMsg(message.clone())));
            }

            // Propagate the message to everyone else who is subscribed to any
            // of the topics.
            for (peer_id, subscr_topics) in self.connected_peers.iter() {
                if peer_id == &propagation_source {
                    continue;
                }

                if !subscr_topics.iter().any(|t| message.topics.iter()
                    .any(|u| t == u.0)) {
                    continue;
                }

                if let Some(pos) = rpcs_to_dispatch.iter()
                    .position(|(p, _)| p == peer_id) {
                    rpcs_to_dispatch[pos].1.messages.push(message.clone());
                } else {
                    rpcs_to_dispatch.push((peer_id.clone(), GossipsubRpc {
                        subscriptions: Vec::new(),
                        messages: vec![message.clone()],
                        control: None,
                    }));
                }
            }
        }
        // let mut ctrl = event.control;
        if let Some(ctrl) = event.control {
            // Add the control message to be dispatched to the user. We should
            // only get a control message for a topic if we are subscribed to
            // it, IIUIC. If this were not the case, we would have to check
            // each topic in the control message to see that we are subscribed
            // to it.
            self.events.push_back(NetworkBehaviourAction::GenerateEvent
                (GOutEvents::CtrlMsg(ctrl.clone())));

            // Propagate the control message to everyone else who is
            // subscribed to any of the topics.
            for (peer_id, subscr_topics) in self.connected_peers.iter() {
                if peer_id == &propagation_source {
                    continue;
                }

                // Again, I'm assuming that the peer is already subscribed to // any topics in the control message.
                // TODO: double-check this.

                if let Some(pos) = rpcs_to_dispatch.iter()
                    .position(|(p, _)| p == peer_id) {
                    rpcs_to_dispatch[pos].1.control = Some(ctrl.clone());
                } else {
                    rpcs_to_dispatch.push((peer_id.clone(), GossipsubRpc {
                        subscriptions: Vec::new(),
                        messages: Vec::new(),
                        control: Some(ctrl.clone()),
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
