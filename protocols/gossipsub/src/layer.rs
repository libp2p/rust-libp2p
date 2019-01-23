// Copyright 2018 Parity Technologies (UK) Ltd.
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
//

// TODO: Implement control message piggybacking

use cuckoofilter::CuckooFilter;
use futures::prelude::*;
use handler::GossipsubHandler;
use libp2p_core::swarm::{
    ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
};
use libp2p_core::{protocols_handler::ProtocolsHandler, PeerId};
use libp2p_floodsub::{Topic, TopicHash};
use mcache::MessageCache;
use protocol::{
    GossipsubControlAction, GossipsubMessage, GossipsubRpc, GossipsubSubscription,
    GossipsubSubscriptionAction,
};
use rand;
use rand::{seq::SliceRandom, thread_rng};
use smallvec::SmallVec;
use std::collections::hash_map::{DefaultHasher, HashMap};
use std::time::{Duration, Instant};
use std::{collections::VecDeque, iter, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Interval;

// potentially rename this struct - due to clashes
/// Configuration parameters that define the performance of the gossipsub network.
#[derive(Clone)]
pub struct GossipsubConfig {
    /// Overlay network parameters.
    /// Number of heartbeats to keep in the `memcache`.
    history_length: usize,
    /// Number of past heartbeats to gossip about.
    history_gossip: usize,

    /// Target number of peers for the mesh network (D in the spec).
    mesh_n: usize,
    /// Minimum number of peers in mesh network before adding more (D_lo in the spec).
    mesh_n_low: usize,
    /// Maximum number of peers in mesh network before removing some (D_high in the spec).
    mesh_n_high: usize,

    /// Number of peers to emit gossip to during a heartbeat (D_lazy in the spec).
    gossip_lazy: usize,

    //    Not applicable right now
    //    /// Initial delay in each heartbeat.
    //    heartbeat_initial_delay: Duration,
    /// Time between each heartbeat.
    heartbeat_interval: Duration,
    /// Time to live for fanout peers.
    fanout_ttl: Duration,
}

impl Default for GossipsubConfig {
    fn default() -> GossipsubConfig {
        GossipsubConfig {
            history_length: 5,
            history_gossip: 3,
            mesh_n: 6,
            mesh_n_low: 4,
            mesh_n_high: 12,
            gossip_lazy: 6, // default to mesh_n
            //            heartbeat_initial_delay: Duration::from_millis(100),
            heartbeat_interval: Duration::from_secs(1),
            fanout_ttl: Duration::from_secs(60),
        }
    }
}

impl GossipsubConfig {
    pub fn new(
        history_length: usize,
        history_gossip: usize,
        mesh_n: usize,
        mesh_n_low: usize,
        mesh_n_high: usize,
        gossip_lazy: usize,
        //        heartbeat_initial_delay: Duration,
        heartbeat_interval: Duration,
        fanout_ttl: Duration,
    ) -> GossipsubConfig {
        assert!(
            history_length >= history_gossip,
            "The history_length must be greater than or equal to the history_gossip length"
        );
        assert!(
            mesh_n_low <= mesh_n && mesh_n <= mesh_n_high,
            "The following equality doesn't hold mesh_n_low <= mesh_n <= mesh_n_high"
        );
        GossipsubConfig {
            history_length,
            history_gossip,
            mesh_n,
            mesh_n_low,
            mesh_n_high,
            gossip_lazy,
            // heartbeat_initial_delay,
            heartbeat_interval,
            fanout_ttl,
        }
    }
}

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct Gossipsub<TSubstream> {
    /// Configuration providing gossipsub performance parameters.
    config: GossipsubConfig,

    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, GossipsubEvent>>,

    /// Peer id of the local node. Used for the source of the messages that we publish.
    local_peer_id: PeerId,

    /// A map of all connected peers - A map of topic hash to a tuple containing a list of gossipsub peers and floodsub peers respectively.
    topic_peers: HashMap<TopicHash, (Vec<PeerId>, Vec<PeerId>)>,

    /// A map of all connected peers to a tuple containing their subscribed topics and NodeType
    /// respectively.
    // This is used to efficiently keep track of all currently connected nodes and their type
    peer_topics: HashMap<PeerId, (SmallVec<[TopicHash; 16]>, NodeType)>,

    /// Overlay network of connected peers - Maps topics to connected gossipsub peers.
    mesh: HashMap<TopicHash, Vec<PeerId>>,

    /// Map of topics to list of peers that we publish to, but don't subscribe to.
    fanout: HashMap<TopicHash, Vec<PeerId>>,

    /// The last publish time for fanout topics.
    fanout_last_pub: HashMap<TopicHash, Instant>,

    /// Message cache for the last few heartbeats.
    mcache: MessageCache,

    // We keep track of the messages we received (in the format `string(source ID, seq_no)`) so that
    // we don't dispatch the same message twice if we receive it twice on the network.
    received: CuckooFilter<DefaultHasher>,

    /// Heartbeat interval stream.
    heartbeat: Interval,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Gossipsub<TSubstream> {
    /// Creates a `Gossipsub` struct given a set of parameters specified by `gs_config`.
    pub fn new(local_peer_id: PeerId, gs_config: GossipsubConfig) -> Self {
        Gossipsub {
            config: gs_config.clone(),
            events: VecDeque::new(),
            local_peer_id,
            topic_peers: HashMap::new(),
            peer_topics: HashMap::new(),
            mesh: HashMap::new(),
            fanout: HashMap::new(),
            fanout_last_pub: HashMap::new(),
            mcache: MessageCache::new(gs_config.history_gossip, gs_config.history_length),
            received: CuckooFilter::new(),
            heartbeat: Interval::new_interval(gs_config.heartbeat_interval),
            marker: PhantomData,
        }
    }

    /// Subscribes to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        if self.mesh.get(&topic.hash()).is_some() {
            return false;
        }

        // send subscription request to all floodsub and gossipsub peers
        for (flood_peers, gossip_peers) in self.topic_peers.values() {
            for peer in flood_peers.iter().chain(gossip_peers) {
                self.events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: peer.clone(),
                    event: GossipsubRpc {
                        messages: Vec::new(),
                        subscriptions: vec![GossipsubSubscription {
                            topic_hash: topic.hash().clone(),
                            action: GossipsubSubscriptionAction::Subscribe,
                        }],
                        control_msgs: Vec::new(),
                    },
                });
            }
        }

        // call JOIN(topic)
        // this will add new peers to the mesh for the topic
        self.join(topic);

        true
    }

    /// Unsubscribes from a topic.
    ///
    /// Note that this only requires a `TopicHash` and not a full `Topic`.
    ///
    /// Returns true if we were subscribed to this topic.
    pub fn unsubscribe(&mut self, topic: impl AsRef<TopicHash>) -> bool {
        let topic_hash = topic.as_ref();

        if self.mesh.get(topic_hash).is_none() {
            // we are not subscribed
            return false;
        }

        // announce to all floodsub and gossipsub peers
        for (flood_peers, gossip_peers) in self.topic_peers.values() {
            for peer in flood_peers.iter().chain(gossip_peers) {
                self.events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: peer.clone(),
                    event: GossipsubRpc {
                        messages: Vec::new(),
                        subscriptions: vec![GossipsubSubscription {
                            topic_hash: topic_hash.clone(),
                            action: GossipsubSubscriptionAction::Unsubscribe,
                        }],
                        control_msgs: Vec::new(),
                    },
                });
            }
        }

        // call LEAVE(topic)
        // this will remove the topic from the mesh
        self.leave(&topic);

        true
    }

    /// Publishes a message to the network.
    pub fn publish(&mut self, topic: impl Into<TopicHash>, data: impl Into<Vec<u8>>) {
        self.publish_many(iter::once(topic), data)
    }

    /// Publishes a message with multiple topics to the network.
    pub fn publish_many(
        &mut self,
        topic: impl IntoIterator<Item = impl Into<TopicHash>>,
        data: impl Into<Vec<u8>>,
    ) {
        let message = GossipsubMessage {
            source: self.local_peer_id.clone(),
            data: data.into(),
            // If the sequence numbers are predictable, then an attacker could flood the network
            // with packets with the predetermined sequence numbers and absorb our legitimate
            // messages. We therefore use a random number.
            // TODO: Check if the random sequence numbers causes issues with other clients.
            // To be interoperable with the go-implementation this is treated as a 64-bit
            // big-endian uint.
            sequence_number: rand::random::<[u8; 8]>().to_vec(),
            topics: topic.into_iter().map(|t| t.into().clone()).collect(),
        };

        // forward the message to mesh and floodsub peers
        let local_peer_id = self.local_peer_id.clone();
        self.forward_msg(message.clone(), local_peer_id);

        let mut recipient_peers = HashMap::new();
        for topic_hash in &message.topics {
            // if not subscribed to the topic, use fanout peers
            if self.mesh.get(&topic_hash).is_none() {
                // build a list of peers to forward the message to
                // if we have fanout peers add them to the map
                if let Some(fanout_peers) = self.fanout.get(&topic_hash) {
                    for peer in fanout_peers {
                        recipient_peers.insert(peer.clone(), ());
                    }
                }
            } else {
                // TODO: Ensure fanout key never contains an empty set
                // we have no fanout peers, select mesh_n of them and add them to the fanout
                let mesh_n = self.config.mesh_n;
                let new_peers = self.get_random_peers(&topic_hash, mesh_n, { |_| true });
                // add the new peers to the fanout and recipient peers
                self.fanout.insert(topic_hash.clone(), new_peers.clone());
                for peer in new_peers {
                    recipient_peers.insert(peer.clone(), ());
                }
            }
            // we are publishing to fanout peers - update the time we published
            self.fanout_last_pub
                .insert(topic_hash.clone(), Instant::now());
        }

        // add published message to our received caches
        self.mcache.put(message.clone());
        self.received.add(&message.msg_id());

        // Send to peers we know are subscribed to the topic.
        for peer_id in recipient_peers.keys() {
            println!("peers subscribed? {:?}", peer_id);
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: vec![message.clone()],
                    control_msgs: Vec::new(),
                },
            });
        }
    }

    /// Gossipsub JOIN(topic) - adds topic peers to mesh and sends them GRAFT messages.
    fn join(&mut self, topic: impl AsRef<TopicHash>) {
        let topic_hash = topic.as_ref();

        // if we are already in the mesh, return
        if self.mesh.contains_key(topic_hash) {
            return;
        }

        let mut peers = vec![];

        // check if we have peers in fanout[topic] and remove them if we do
        if let Some((_, peers)) = self.fanout.remove_entry(topic_hash) {
            // add them to the mesh
            self.mesh.insert(topic_hash.clone(), peers.clone());
            // remove the last published time
            self.fanout_last_pub.remove(topic_hash);
        } else {
            // no peers in fanout[topic] - select mesh_n at random
            let mesh_n = self.config.mesh_n;
            peers = self.get_random_peers(topic_hash, mesh_n, { |_| true });
            // put them in the mesh
            self.mesh.insert(topic_hash.clone(), peers.clone());
        }

        for peer_id in peers {
            // Send a GRAFT control message
            println!("Graft message sent to peer: {:?}", peer_id);
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: vec![GossipsubControlAction::Graft {
                        topic_hash: topic_hash.clone(),
                    }],
                },
            });
            //TODO: tagPeer
        }
    }

    /// Gossipsub LEAVE(topic) - Notifies mesh[topic] peers with PRUNE messages.
    fn leave(&mut self, topic: impl AsRef<TopicHash>) {
        let topic_hash = topic.as_ref();

        // if our mesh contains the topic, send prune to peers and delete it from the mesh
        if let Some((_, peers)) = self.mesh.remove_entry(topic_hash) {
            for peer_id in peers {
                // Send a PRUNE control message
                println!("Prune message sent to peer: {:?}", peer_id);
                self.events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: peer_id.clone(),
                    event: GossipsubRpc {
                        subscriptions: Vec::new(),
                        messages: Vec::new(),
                        control_msgs: vec![GossipsubControlAction::Prune {
                            topic_hash: topic_hash.clone(),
                        }],
                    },
                });
                //TODO: untag Peer
            }
        }
    }

    /// Handles an IHAVE control message. Checks our cache of messages. If the message is unknown,
    /// requests it with an IWANT control message.
    fn handle_ihave(&mut self, peer_id: &PeerId, ihave_msgs: Vec<(TopicHash, Vec<String>)>) {
        // use a hashmap to avoid duplicates efficiently
        let mut iwant_msg_ids = HashMap::new();

        for (topic, msg_ids) in ihave_msgs {
            // only process the message if we are subscribed
            if !self.mesh.contains_key(&topic) {
                return; // continue
            }

            for msg_id in msg_ids {
                if !self.received.contains(&msg_id) {
                    // have not seen this message, request it
                    iwant_msg_ids.insert(msg_id, true);
                }
            }
        }

        if !iwant_msg_ids.is_empty() {
            // Send the list of IWANT control messages
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: vec![GossipsubControlAction::IWant {
                        message_ids: iwant_msg_ids.keys().map(|msg_id| msg_id.clone()).collect(),
                    }],
                },
            });
        }
    }

    /// Handles an IWANT control message. Checks our cache of messages. If the message exists it is
    /// forwarded to the requesting peer.
    fn handle_iwant(&mut self, peer_id: &PeerId, iwant_msgs: Vec<String>) {
        // build a hashmap of available messages
        let mut cached_messages = HashMap::new();

        for msg_id in iwant_msgs {
            // if we have it, add it do the cached_messages mapping
            if let Some(msg) = self.mcache.get(&msg_id) {
                cached_messages.insert(msg_id.clone(), msg.clone());
            }
        }

        if !cached_messages.is_empty() {
            // Send the messages to the peer
            let message_list = cached_messages.values().map(|msg| msg.clone()).collect();
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: message_list,
                    control_msgs: Vec::new(),
                },
            });
        }
    }

    /// Handles GRAFT control messages. If subscribed to the topic, adds the peer to mesh, if not, responds
    /// with PRUNE messages.
    fn handle_graft(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        let mut to_prune_topics = HashMap::new();
        for topic_hash in topics {
            if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                // if we are subscribed, add peer to the mesh
                println!("GRAFT: Mesh link added from {:?}", peer_id);
                peers.push(peer_id.clone());
            //TODO: tagPeer
            } else {
                to_prune_topics.insert(topic_hash.clone(), ());
            }
        }

        if !to_prune_topics.is_empty() {
            // build the prune messages to send
            let prune_messages = to_prune_topics
                .keys()
                .map(|topic_hash| GossipsubControlAction::Prune {
                    topic_hash: topic_hash.clone(),
                })
                .collect();
            // Send the prune messages to the peer
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: prune_messages,
                },
            });
        }
    }

    /// Handles PRUNE control messages. Removes peer from the mesh.
    fn handle_prune(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        for topic_hash in topics {
            if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                // remove the peer if it exists in the mesh
                if let Some(pos) = peers.iter().position(|p| p == peer_id) {
                    peers.remove(pos);
                    //TODO: untagPeer
                }
            }
        }
    }

    /// Handles a newly received GossipsubMessage.
    /// Forwards the message to all floodsub peers and peers in the mesh.
    fn handle_received_message(&mut self, msg: GossipsubMessage, propagation_source: &PeerId) {
        // if we have seen this message, ignore it
        // there's a 3% chance this is a false positive
        // TODO: Check this has no significant emergent behaviour
        if !self.received.test_and_add(&msg.msg_id()) {
            return;
        }

        // add to the memcache
        self.mcache.put(msg.clone());

        // dispatch the message to the user
        if self.mesh.keys().any(|t| msg.topics.iter().any(|u| t == u)) {
            self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                GossipsubEvent::Message(msg.clone()),
            ));
        }

        // forward the message to floodsub and mesh peers
        self.forward_msg(msg, propagation_source.clone());
    }

    /// Handles received subscriptions.
    fn handle_received_subscriptions(
        &mut self,
        subscriptions: Vec<GossipsubSubscription>,
        propagation_source: &PeerId,
    ) {
        let (subscribed_topics, node_type) = match self.peer_topics.get_mut(&propagation_source) {
            Some((topics, node_type)) => (topics, node_type),
            None => {
                println!(
                    "ERROR: Subscription by unknown peer: {:?}",
                    &propagation_source
                );
                return;
            }
        };

        for subscription in subscriptions {
            // get the peers from the mapping, or insert empty lists if topic doesn't exist
            let (flood_peers, gossip_peers) = self
                .topic_peers
                .entry(subscription.topic_hash.clone())
                .or_insert((vec![], vec![]));

            match subscription.action {
                GossipsubSubscriptionAction::Subscribe => {
                    match node_type {
                        NodeType::Floodsub => {
                            if !flood_peers.contains(&propagation_source) {
                                flood_peers.push(propagation_source.clone());
                            }
                        }
                        NodeType::Gossipsub => {
                            if !gossip_peers.contains(&propagation_source) {
                                gossip_peers.push(propagation_source.clone());
                            }
                        }
                    }
                    // add to the peer_topics mapping
                    if !subscribed_topics.contains(&subscription.topic_hash) {
                        subscribed_topics.push(subscription.topic_hash.clone());
                    }
                    // generates a subscription event to be polled
                    self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                        GossipsubEvent::Subscribed {
                            peer_id: propagation_source.clone(),
                            topic: subscription.topic_hash,
                        },
                    ));
                }
                GossipsubSubscriptionAction::Unsubscribe => {
                    match node_type {
                        NodeType::Floodsub => {
                            if let Some(pos) =
                                flood_peers.iter().position(|p| p == propagation_source)
                            {
                                flood_peers.remove(pos);
                            }
                        }
                        NodeType::Gossipsub => {
                            if let Some(pos) =
                                gossip_peers.iter().position(|p| p == propagation_source)
                            {
                                gossip_peers.remove(pos);
                            }
                        }
                    }
                    // remove topic from the peer_topics mapping
                    if let Some(pos) = subscribed_topics
                        .iter()
                        .position(|t| t == &subscription.topic_hash)
                    {
                        subscribed_topics.remove(pos);
                    }
                    // generate a subscription even to be polled
                    self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                        GossipsubEvent::Unsubscribed {
                            peer_id: propagation_source.clone(),
                            topic: subscription.topic_hash,
                        },
                    ));
                }
            }
        }
    }

    /// Heartbeat function which shifts the memcache and updates the mesh
    fn heartbeat(&mut self) {
        //TODO: Clean up any state from last heartbeat.

        let mut to_graft = HashMap::new();
        let mut to_prune = HashMap::new();

        // maintain the mesh for each topic
        for (topic_hash, peers) in self.mesh.clone().iter_mut() {
            // too little peers - add some
            if peers.len() < self.config.mesh_n_low {
                // not enough peers - get mesh_n - current_length more
                let desired_peers = self.config.mesh_n - peers.len();
                let peer_list = self
                    .get_random_peers(topic_hash, desired_peers, { |peer| !peers.contains(peer) });
                for peer in peer_list {
                    peers.push(peer.clone());
                    // TODO: tagPeer
                    let current_topic = to_graft.entry(peer).or_insert(Vec::new());
                    current_topic.push(topic_hash.clone());
                }
                // update the mesh
                self.mesh.insert(topic_hash.clone(), peers.clone());
            }

            // too many peers - remove some
            if peers.len() > self.config.mesh_n_high {
                let excess_peer_no = peers.len() - self.config.mesh_n;
                // shuffle the peers
                let mut rng = thread_rng();
                peers.shuffle(&mut rng);
                // remove the first excess_peer_no peers adding them to to_prune
                for _ in 0..excess_peer_no {
                    let peer = peers
                        .pop()
                        .expect("There should always be enough peers to remove");
                    let current_topic = to_prune.entry(peer).or_insert(vec![]);
                    current_topic.push(topic_hash.clone());
                    //TODO: untagPeer
                }
                // update the mesh
                self.mesh.insert(topic_hash.clone(), peers.clone());
            }

            // emit gossip
            self.emit_gossip(topic_hash.clone(), peers.clone());
        }

        // remove expired fanout topics
        {
            let fanout = &mut self.fanout; // help the borrow checker
            let fanout_ttl = self.config.fanout_ttl;
            self.fanout_last_pub.retain(|topic_hash, last_pub_time| {
                if *last_pub_time + fanout_ttl < Instant::now() {
                    fanout.remove(&topic_hash);
                    return false;
                }
                true
            });
        }

        // maintain fanout
        // check if our peers are still apart of the topic
        for (topic_hash, peers) in self.fanout.clone().iter_mut() {
            peers.retain(|peer| {
                // is the peer still subscribed to the topic?
                if !self
                    .peer_topics
                    .get(peer)
                    .expect("Peer should exist")
                    .0
                    .contains(&topic_hash)
                {
                    return false;
                }
                true
            });

            // not enough peers
            if peers.len() < self.config.mesh_n {
                let needed_peers = self.config.mesh_n - peers.len();
                let mut new_peers =
                    self.get_random_peers(topic_hash, needed_peers, |peer| !peers.contains(peer));
                peers.append(&mut new_peers);
            }
            // update the entry
            self.fanout.insert(topic_hash.clone(), peers.to_vec());

            self.emit_gossip(topic_hash.clone(), peers.clone());
        }

        // send graft/prunes
        self.send_graft_prune(to_graft, to_prune);

        // shift the memcache
        self.mcache.shift();
    }

    /// Emits gossip - Send IHAVE messages to a random set of gossip peers that are not in the mesh, but are subscribed to the `topic`.
    fn emit_gossip(&mut self, topic_hash: TopicHash, peers: Vec<PeerId>) {
        let message_ids = self.mcache.get_gossip_ids(&topic_hash);
        if message_ids.is_empty() {
            return;
        }

        // get gossip_lazy random peers
        let to_msg_peers = self.get_random_peers(&topic_hash, self.config.gossip_lazy, |peer| {
            !peers.contains(peer)
        });
        for peer in to_msg_peers {
            // send an IHAVE message
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer,
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: vec![GossipsubControlAction::IHave {
                        topic_hash: topic_hash.clone(),
                        message_ids: message_ids.clone(),
                    }],
                },
            });
        }
    }

    /// Handles multiple GRAFT/PRUNE messages and coalesces them into chunked gossip control
    /// messages.
    fn send_graft_prune(
        &mut self,
        to_graft: HashMap<PeerId, Vec<TopicHash>>,
        mut to_prune: HashMap<PeerId, Vec<TopicHash>>,
    ) {
        // handle the grafts and overlapping prunes
        for (peer, topics) in to_graft.iter() {
            let mut grafts: Vec<GossipsubControlAction> = topics
                .iter()
                .map(|topic_hash| {
                    return GossipsubControlAction::Graft {
                        topic_hash: topic_hash.clone(),
                    };
                })
                .collect();
            let mut prunes: Vec<GossipsubControlAction> = to_prune
                .remove(&peer)
                .unwrap_or_else(|| vec![])
                .iter()
                .map(|topic_hash| GossipsubControlAction::Prune {
                    topic_hash: topic_hash.clone(),
                })
                .collect();
            grafts.append(&mut prunes);

            // send the control messages
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: grafts,
                },
            });
        }

        // handle the remaining prunes
        for (peer, topics) in to_prune.iter() {
            let remaining_prunes = topics
                .iter()
                .map(|topic_hash| GossipsubControlAction::Prune {
                    topic_hash: topic_hash.clone(),
                })
                .collect();
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: remaining_prunes,
                },
            });
        }
    }

    /// Helper function to publish and forward messages to floodsub[topic] and mesh[topic] peers.
    fn forward_msg(&mut self, message: GossipsubMessage, source: PeerId) {
        let mut recipient_peers = HashMap::new();

        // add floodsub and mesh peers
        for topic in &message.topics {
            // floodsub
            if let Some((_, floodsub_peers)) = self.topic_peers.get(&topic) {
                for peer_id in floodsub_peers {
                    if *peer_id != source {
                        recipient_peers.insert(peer_id.clone(), ());
                    }
                }
            }

            // mesh
            if let Some(mesh_peers) = self.mesh.get(&topic) {
                for peer_id in mesh_peers {
                    if *peer_id != source {
                        recipient_peers.insert(peer_id.clone(), ());
                    }
                }
            }
        }

        // forward the message to peers
        if !recipient_peers.is_empty() {
            for peer in recipient_peers.keys() {
                self.events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: peer.clone(),
                    event: GossipsubRpc {
                        subscriptions: Vec::new(),
                        messages: vec![message.clone()],
                        control_msgs: Vec::new(),
                    },
                });
            }
        }
    }

    /// Helper function to get a set of `n` random gossipsub peers for a `topic_hash`
    /// filtered by the function `f`.
    fn get_random_peers(
        &self,
        topic_hash: &TopicHash,
        n: usize,
        mut f: impl FnMut(&PeerId) -> bool,
    ) -> Vec<PeerId> {
        let mut gossip_peers = match self.topic_peers.get(topic_hash) {
            // if they exist, filter the peers by `f`
            Some((gossip_peers, _)) => gossip_peers.iter().cloned().filter(|p| f(p)).collect(),
            None => Vec::new(),
        };

        // if we have less than needed, return them
        if gossip_peers.len() <= n {
            return gossip_peers.to_vec();
        }

        // we have more peers than needed, shuffle them and return n of them
        let mut rng = thread_rng();
        gossip_peers.partial_shuffle(&mut rng, n);

        return gossip_peers[..n].to_vec();
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for Gossipsub<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = GossipsubHandler<TSubstream>;
    type OutEvent = GossipsubEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        GossipsubHandler::new()
    }

    fn inject_connected(&mut self, id: PeerId, _: ConnectedPoint) {
        // We need to send our subscriptions to the newly-connected node.
        let mut subscriptions = vec![];
        for topic_hash in self.mesh.keys() {
            subscriptions.push(GossipsubSubscription {
                topic_hash: topic_hash.clone(),
                action: GossipsubSubscriptionAction::Subscribe,
            });
        }

        if !subscriptions.is_empty() {
            // send our subscriptions to the peer
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions,
                    control_msgs: Vec::new(),
                },
            });
        }

        // TODO: Handle the peer addition - Specifically handle floodsub peers.
        // For the time being assume all gossipsub peers
        self.peer_topics
            .insert(id.clone(), (SmallVec::new(), NodeType::Gossipsub));
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        // TODO: Handle peer disconnection - specifically floodsub peers
        // TODO: Refactor
        // remove from mesh, topic_peers and peer_topic
        {
            let (topics, node_type) = match self.peer_topics.get(&id) {
                Some((topics, node_type)) => (topics, node_type),
                None => {
                    println!("ERROR: Disconnected node, not in connected nodes");
                    return;
                }
            };

            // remove peer from all mappings
            for topic in topics {
                // check the mesh for the topic
                if let Some(mesh_peers) = self.mesh.get_mut(&topic) {
                    // check if the peer is in the mesh and remove it
                    if let Some(pos) = mesh_peers.iter().position(|p| p == id) {
                        mesh_peers.remove(pos);
                        //TODO: untagPeer
                    }
                }

                // remove from topic_peers
                if let Some((floodsub_peers, gossip_peers)) = self.topic_peers.get_mut(&topic) {
                    match node_type {
                        NodeType::Gossipsub => {
                            if let Some(pos) = gossip_peers.iter().position(|p| p == id) {
                                gossip_peers.remove(pos);
                            //TODO: untagPeer
                            }
                            // debugging purposes
                            else {
                                println!(
                                    "ERROR: Disconnected node: {:?} not in topic_peers peer list",
                                    &id
                                );
                            }
                        }
                        NodeType::Floodsub => {
                            if let Some(pos) = floodsub_peers.iter().position(|p| p == id) {
                                floodsub_peers.remove(pos);
                            //TODO: untagPeer
                            }
                            // debugging purposes
                            else {
                                println!(
                                    "ERROR: Disconnected node: {:?} not in topic_peers peer list",
                                    &id
                                );
                            }
                        }
                    }
                } else {
                    println!(
                        "ERROR: Disconnected node: {:?} with topic: {:?} not in topic_peers",
                        &id, &topic
                    );
                }
            }
        }

        // remove peer from peer_topics
        let was_in = self.peer_topics.remove(id);
        debug_assert!(was_in.is_some());
    }

    fn inject_node_event(&mut self, propagation_source: PeerId, event: GossipsubRpc) {
        // Handle subscriptions
        // Update connected peers topics
        self.handle_received_subscriptions(event.subscriptions, &propagation_source);

        // Handle messages
        for message in event.messages {
            self.handle_received_message(message, &propagation_source);
        }

        // Handle control messages
        // group some control messages, this minimises SendEvents (code is simplified to handle each event at a time however)
        // TODO: Decide if the grouping is necessary
        let mut ihave_msgs = vec![];
        let mut graft_msgs = vec![];
        let mut prune_msgs = vec![];
        for control_msg in event.control_msgs {
            match control_msg {
                GossipsubControlAction::IHave {
                    topic_hash,
                    message_ids,
                } => {
                    ihave_msgs.push((topic_hash, message_ids));
                }
                GossipsubControlAction::IWant { message_ids } => {
                    self.handle_iwant(&propagation_source, message_ids)
                }
                GossipsubControlAction::Graft { topic_hash } => graft_msgs.push(topic_hash),
                GossipsubControlAction::Prune { topic_hash } => prune_msgs.push(topic_hash),
            }
        }
        if !ihave_msgs.is_empty() {
            self.handle_ihave(&propagation_source, ihave_msgs);
        }
        if !graft_msgs.is_empty() {
            self.handle_graft(&propagation_source, graft_msgs);
        }
        if !prune_msgs.is_empty() {
            self.handle_prune(&propagation_source, prune_msgs);
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

        match self.heartbeat.poll() {
            // heartbeat ready
            Ok(Async::Ready(Some(_))) => self.heartbeat(),
            _ => {}
        };

        Async::NotReady
    }
}
/// Event that can happen on the gossipsub behaviour.
#[derive(Debug)]
pub enum GossipsubEvent {
    /// A message has been received.
    Message(GossipsubMessage),

    /// A remote subscribed to a topic.
    Subscribed {
        /// Remote that has subscribed.
        peer_id: PeerId,
        /// The topic it has subscribed to.
        topic: TopicHash,
    },

    /// A remote unsubscribed from a topic.
    Unsubscribed {
        /// Remote that has unsubscribed.
        peer_id: PeerId,
        /// The topic it has subscribed from.
        topic: TopicHash,
    },
}

/// The type of node in the pubsub system.
pub enum NodeType {
    /// A gossipsub node.
    Gossipsub,
    /// A Floodsub node.
    Floodsub,
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_floodsub::TopicBuilder;

    #[test]
    /// Test Gossipsub.get_random_peers() function
    fn test_get_random_peers() {
        // generate a default GossipsubConfig
        let gs_config = GossipsubConfig::default();
        // create a gossipsub struct
        let mut gs: Gossipsub<usize> = Gossipsub::new(PeerId::random(), gs_config);

        // create a topic and fill it with some peers
        let topic_hash = TopicBuilder::new("Test").build().hash().clone();
        let mut peers = vec![];
        for _ in 0..20 {
            peers.push(PeerId::random())
        }

        gs.topic_peers
            .insert(topic_hash.clone(), (peers.clone(), vec![]));

        let random_peers = gs.get_random_peers(&topic_hash, 5, { |_| true });
        assert!(random_peers.len() == 5, "Expected 5 peers to be returned");
        let random_peers = gs.get_random_peers(&topic_hash, 30, { |_| true });
        assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
        assert!(random_peers == peers, "Expected no shuffling");
        let random_peers = gs.get_random_peers(&topic_hash, 20, { |_| true });
        assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
        assert!(random_peers == peers, "Expected no shuffling");
        let random_peers = gs.get_random_peers(&topic_hash, 0, { |_| true });
        assert!(random_peers.len() == 0, "Expected 0 peers to be returned");
    }

}
