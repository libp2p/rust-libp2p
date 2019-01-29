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
use libp2p_core::swarm::{
    ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
};
use libp2p_core::{
    protocols_handler::{OneShotHandler, ProtocolsHandler},
    PeerId,
};
use libp2p_floodsub::{Topic, TopicHash};
use mcache::MessageCache;
use protocol::{
    GossipsubControlAction, GossipsubMessage, GossipsubRpc, GossipsubSubscription,
    GossipsubSubscriptionAction, ProtocolConfig,
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

    /// Initial delay in each heartbeat.
    heartbeat_initial_delay: Duration,
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
            heartbeat_initial_delay: Duration::from_secs(5),
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
        heartbeat_initial_delay: Duration,
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
            heartbeat_initial_delay,
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
            heartbeat: Interval::new(
                Instant::now() + gs_config.heartbeat_initial_delay,
                gs_config.heartbeat_interval,
            ),
            marker: PhantomData,
        }
    }

    /// Subscribes to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        debug!("Subscribing to topic: {:?}", topic);
        if self.mesh.get(&topic.hash()).is_some() {
            debug!("Topic: {:?} is already in the mesh.", topic);
            return false;
        }

        // send subscription request to all floodsub and gossipsub peers
        for (gossip_peers, flood_peers) in self.topic_peers.get(&topic.hash()) {
            for peer in flood_peers.iter().chain(gossip_peers) {
                debug!("Sending SUBSCRIBE to peer: {:?}", peer);
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
        self.join(topic.hash());
        info!("Subscribed to topic: {:?}", topic);
        true
    }

    /// Unsubscribes from a topic.
    ///
    /// Note that this only requires a `TopicHash` and not a full `Topic`.
    ///
    /// Returns true if we were subscribed to this topic.
    pub fn unsubscribe(&mut self, topic: impl AsRef<TopicHash>) -> bool {
        let topic_hash = topic.as_ref();
        debug!("Unsubscribing from topic: {:?}", topic_hash);

        if self.mesh.get(topic_hash).is_none() {
            debug!("Already unsubscribed to topic: {:?}", topic_hash);
            // we are not subscribed
            return false;
        }

        // announce to all floodsub and gossipsub peers, in the topic
        for (gossip_peers, flood_peers) in self.topic_peers.get(topic_hash) {
            for peer in flood_peers.iter().chain(gossip_peers) {
                debug!("Sending UNSUBSCRIBE to peer: {:?}", peer);
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
        self.leave(&topic_hash);

        info!("Unsubscribed from topic: {:?}", topic_hash);
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

        debug!("Publishing message: {:?}", message.msg_id());

        // forward the message to mesh and floodsub peers
        let local_peer_id = self.local_peer_id.clone();
        self.forward_msg(message.clone(), local_peer_id);

        let mut recipient_peers = HashMap::new();
        for topic_hash in &message.topics {
            // if not subscribed to the topic, use fanout peers
            if self.mesh.get(&topic_hash).is_none() {
                debug!("Topic: {:?} not in the mesh", topic_hash);
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
                    debug!("Peer added to fanout: {:?}", peer);
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
            debug!("Sending message to peer: {:?}", peer_id);
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: vec![message.clone()],
                    control_msgs: Vec::new(),
                },
            });
        }
        info!("Published message: {:?}", message.msg_id());
    }

    /// Gossipsub JOIN(topic) - adds topic peers to mesh and sends them GRAFT messages.
    fn join(&mut self, topic_hash: &TopicHash) {
        debug!("Running JOIN for topic: {:?}", topic_hash);

        // if we are already in the mesh, return
        if self.mesh.contains_key(topic_hash) {
            info!("JOIN: The topic is already in the mesh, ignoring JOIN");
            return;
        }

        let mut added_peers = vec![];

        // check if we have mesh_n peers in fanout[topic] and add them to the mesh if we do,
        // removing the fanout entry.
        if let Some((_, peers)) = self.fanout.remove_entry(topic_hash) {
            debug!(
                "JOIN: Removing peers from the fanout for topic: {:?}",
                topic_hash
            );
            // add up to mesh_n of them them to the mesh
            // Note: These aren't randomly added, currently FIFO
            let mut add_peers = self.config.mesh_n.clone();
            if peers.len() < self.config.mesh_n {
                add_peers = peers.len();
            }
            debug!(
                "JOIN: Adding {:?} peers from the fanout for topic: {:?}",
                add_peers, topic_hash
            );
            added_peers.append(&mut peers.clone()[..add_peers].to_vec());
            self.mesh
                .insert(topic_hash.clone(), peers[..add_peers].to_vec());
            // remove the last published time
            self.fanout_last_pub.remove(topic_hash);
        }

        // check if we need to get more peers, which we randomly select
        if added_peers.len() < self.config.mesh_n {
            // get the peers
            let mut new_peers =
                self.get_random_peers(topic_hash, self.config.mesh_n - added_peers.len(), {
                    |_| true
                });
            added_peers.append(&mut new_peers.clone());
            // add them to the mesh
            debug!(
                "JOIN: Inserting {:?} random peers into the mesh",
                new_peers.len()
            );
            let mesh_peers = self.mesh.entry(topic_hash.clone()).or_insert(vec![]);
            mesh_peers.append(&mut new_peers);
        }

        for peer_id in added_peers {
            // Send a GRAFT control message
            info!("JOIN: Sending Graft message to peer: {:?}", peer_id);
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
        debug!("Completed JOIN for topic: {:?}", topic_hash);
    }

    /// Gossipsub LEAVE(topic) - Notifies mesh[topic] peers with PRUNE messages.
    fn leave(&mut self, topic_hash: &TopicHash) {
        debug!("Running LEAVE for topic {:?}", topic_hash);

        // if our mesh contains the topic, send prune to peers and delete it from the mesh
        if let Some((_, peers)) = self.mesh.remove_entry(topic_hash) {
            for peer in peers {
                // Send a PRUNE control message
                info!("LEAVE: Sending PRUNE to peer: {:?}", peer);
                self.events.push_back(NetworkBehaviourAction::SendEvent {
                    peer_id: peer.clone(),
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
        debug!("Completed LEAVE for topic: {:?}", topic_hash);
    }

    /// Handles an IHAVE control message. Checks our cache of messages. If the message is unknown,
    /// requests it with an IWANT control message.
    fn handle_ihave(&mut self, peer_id: &PeerId, ihave_msgs: Vec<(TopicHash, Vec<String>)>) {
        debug!("Handling IHAVE for peer: {:?}", peer_id);
        // use a hashmap to avoid duplicates efficiently
        let mut iwant_msg_ids = HashMap::new();

        for (topic, msg_ids) in ihave_msgs {
            // only process the message if we are subscribed
            if !self.mesh.contains_key(&topic) {
                info!(
                    "IHAVE: Ignoring IHAVE - Not subscribed to topic: {:?}",
                    topic
                );
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
            info!("IHAVE: Sending IWANT message");
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: vec![GossipsubControlAction::IWant {
                        message_ids: iwant_msg_ids.keys().cloned().collect(),
                    }],
                },
            });
        }
        debug!("Completed IHAVE handling for peer: {:?}", peer_id);
    }

    /// Handles an IWANT control message. Checks our cache of messages. If the message exists it is
    /// forwarded to the requesting peer.
    fn handle_iwant(&mut self, peer_id: &PeerId, iwant_msgs: Vec<String>) {
        debug!("Handling IWANT for peer: {:?}", peer_id);
        // build a hashmap of available messages
        let mut cached_messages = HashMap::new();

        for msg_id in iwant_msgs {
            // if we have it, add it do the cached_messages mapping
            if let Some(msg) = self.mcache.get(&msg_id) {
                cached_messages.insert(msg_id.clone(), msg.clone());
            }
        }

        if !cached_messages.is_empty() {
            info!("IWANT: Sending cached messages to peer: {:?}", peer_id);
            // Send the messages to the peer
            let message_list = cached_messages.values().cloned().collect();
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: message_list,
                    control_msgs: Vec::new(),
                },
            });
        }
        debug!("Completed IWANT handling for peer: {:?}", peer_id);
    }

    /// Handles GRAFT control messages. If subscribed to the topic, adds the peer to mesh, if not, responds
    /// with PRUNE messages.
    fn handle_graft(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        debug!("Handling GRAFT message for peer: {:?}", peer_id);

        let mut to_prune_topics = HashMap::new();
        for topic_hash in topics {
            if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                // if we are subscribed, add peer to the mesh
                info!(
                    "GRAFT: Mesh link added for peer: {:?} in topic: {:?}",
                    peer_id, topic_hash
                );
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
                .map(|t| GossipsubControlAction::Prune {
                    topic_hash: t.clone(),
                })
                .collect();
            // Send the prune messages to the peer
            info!(
                "GRAFT: Not subscribed to topics -  Sending PRUNE to peer: {:?}",
                peer_id
            );
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: Vec::new(),
                    control_msgs: prune_messages,
                },
            });
        }
        debug!("Completed GRAFT handling for peer: {:?}", peer_id);
    }

    /// Handles PRUNE control messages. Removes peer from the mesh.
    fn handle_prune(&mut self, peer_id: &PeerId, topics: Vec<TopicHash>) {
        debug!("Handling PRUNE message for peer: {:?}", peer_id);
        for topic_hash in topics {
            if let Some(peers) = self.mesh.get_mut(&topic_hash) {
                // remove the peer if it exists in the mesh
                info!(
                    "PRUNE: Removing peer: {:?} from the mesh for topic: {:?}",
                    peer_id, topic_hash
                );
                peers.retain(|p| p != peer_id);
                //TODO: untagPeer
            }
        }
        debug!("Completed PRUNE handling for peer: {:?}", peer_id);
    }

    /// Handles a newly received GossipsubMessage.
    /// Forwards the message to all floodsub peers and peers in the mesh.
    fn handle_received_message(&mut self, msg: GossipsubMessage, propagation_source: &PeerId) {
        debug!(
            "Handling message: {:?} from peer: {:?}",
            msg.msg_id(),
            propagation_source
        );
        // if we have seen this message, ignore it
        // there's a 3% chance this is a false positive
        // TODO: Check this has no significant emergent behaviour
        if !self.received.test_and_add(&msg.msg_id()) {
            info!(
                "Message already received, ignoring. Message: {:?}",
                msg.msg_id()
            );
            return;
        }

        // add to the memcache
        self.mcache.put(msg.clone());

        // dispatch the message to the user
        if self.mesh.keys().any(|t| msg.topics.iter().any(|u| t == u)) {
            debug!("Sending received message to poll");
            self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                GossipsubEvent::Message(msg.clone()),
            ));
        }

        // forward the message to floodsub and mesh peers
        self.forward_msg(msg.clone(), propagation_source.clone());
        debug!("Completed message handling for message: {:?}", msg.msg_id());
    }

    /// Handles received subscriptions.
    fn handle_received_subscriptions(
        &mut self,
        subscriptions: &Vec<GossipsubSubscription>,
        propagation_source: &PeerId,
    ) {
        debug!(
            "Handling subscriptions from source: {:?}",
            propagation_source
        );
        let (subscribed_topics, node_type) = match self.peer_topics.get_mut(&propagation_source) {
            Some((topics, node_type)) => (topics, node_type),
            None => {
                error!("Subscription by unknown peer: {:?}", &propagation_source);
                return;
            }
        };

        for subscription in subscriptions {
            // get the peers from the mapping, or insert empty lists if topic doesn't exist
            let (gossip_peers, flood_peers) = self
                .topic_peers
                .entry(subscription.topic_hash.clone())
                .or_insert((vec![], vec![]));

            match subscription.action {
                GossipsubSubscriptionAction::Subscribe => {
                    match node_type {
                        NodeType::Floodsub => {
                            if !flood_peers.contains(&propagation_source) {
                                debug!("SUBSCRIPTION: topic_peer: Adding floodsub peer: {:?} to topic: {:?}", propagation_source, subscription.topic_hash);
                                flood_peers.push(propagation_source.clone());
                            }
                        }
                        NodeType::Gossipsub => {
                            if !gossip_peers.contains(&propagation_source) {
                                debug!("SUBSCRIPTION: topic_peer: Adding gossip peer: {:?} to topic: {:?}", propagation_source, subscription.topic_hash);
                                gossip_peers.push(propagation_source.clone());
                            }
                        }
                    }
                    // add to the peer_topics mapping
                    if !subscribed_topics.contains(&subscription.topic_hash) {
                        info!(
                            "SUBSCRIPTION: Adding peer: {:?} to topic: {:?}",
                            propagation_source, subscription.topic_hash
                        );
                        subscribed_topics.push(subscription.topic_hash.clone());
                    }
                    // generates a subscription event to be polled
                    self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                        GossipsubEvent::Subscribed {
                            peer_id: propagation_source.clone(),
                            topic: subscription.topic_hash.clone(),
                        },
                    ));
                }
                GossipsubSubscriptionAction::Unsubscribe => {
                    match node_type {
                        NodeType::Floodsub => {
                            if let Some(pos) =
                                flood_peers.iter().position(|p| p == propagation_source)
                            {
                                info!(
                                    "SUBSCRIPTION: Removing floodsub peer: {:?} from topic: {:?}",
                                    propagation_source, subscription.topic_hash
                                );
                                flood_peers.remove(pos);
                            }
                        }
                        NodeType::Gossipsub => {
                            if let Some(pos) =
                                gossip_peers.iter().position(|p| p == propagation_source)
                            {
                                info!(
                                    "SUBSCRIPTION: Removing gossip peer: {:?} from topic: {:?}",
                                    propagation_source, subscription.topic_hash
                                );
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
                            topic: subscription.topic_hash.clone(),
                        },
                    ));
                }
            }
        }
        debug!(
            "Completed handling subscriptions from source: {:?}",
            propagation_source
        );
    }

    /// Heartbeat function which shifts the memcache and updates the mesh
    fn heartbeat(&mut self) {
        debug!("Starting heartbeat");

        //TODO: Clean up any state from last heartbeat.

        let mut to_graft = HashMap::new();
        let mut to_prune = HashMap::new();

        // maintain the mesh for each topic
        for (topic_hash, peers) in self.mesh.clone().iter_mut() {
            // too little peers - add some
            if peers.len() < self.config.mesh_n_low {
                debug!(
                    "HEARTBEAT: Mesh low. Topic: {:?} Contains: {:?} needs: {:?}",
                    topic_hash.clone().into_string(),
                    peers.len(),
                    self.config.mesh_n_low
                );
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
                debug!(
                    "HEARTBEAT: Mesh high. Topic: {:?} Contains: {:?} needs: {:?}",
                    topic_hash,
                    peers.len(),
                    self.config.mesh_n_high
                );
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
                    debug!(
                        "HEARTBEAT: Fanout topic removed due to timeout. Topic: {:?}",
                        topic_hash
                    );
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
                    debug!(
                        "HEARTBEAT: Peer removed from fanout for topic: {:?}",
                        topic_hash
                    );
                    return false;
                }
                true
            });

            // not enough peers
            if peers.len() < self.config.mesh_n {
                debug!(
                    "HEARTBEAT: Fanout low. Contains: {:?} needs: {:?}",
                    peers.len(),
                    self.config.mesh_n
                );
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
        if !to_graft.is_empty() | !to_prune.is_empty() {
            self.send_graft_prune(to_graft, to_prune);
        }

        // shift the memcache
        self.mcache.shift();
        debug!("Completed Heartbeat");
    }

    /// Emits gossip - Send IHAVE messages to a random set of gossip peers that are not in the mesh, but are subscribed to the `topic`.
    fn emit_gossip(&mut self, topic_hash: TopicHash, peers: Vec<PeerId>) {
        debug!("Started gossip");
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
        debug!("Completed gossip");
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
        debug!("Forwarding message: {:?}", message.msg_id());
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
                debug!("Sending message: {:?} to peer {:?}", message.msg_id(), peer);
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
        debug!("Completed forwarding message");
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
            debug!("RANDOM PEERS: Got {:?} peers", gossip_peers.len());
            return gossip_peers.to_vec();
        }

        // we have more peers than needed, shuffle them and return n of them
        let mut rng = thread_rng();
        gossip_peers.partial_shuffle(&mut rng, n);

        debug!("RANDOM PEERS: Got {:?} peers", n);

        return gossip_peers[..n].to_vec();
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for Gossipsub<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = OneShotHandler<TSubstream, ProtocolConfig, GossipsubRpc, InnerMessage>;
    type OutEvent = GossipsubEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        Default::default()
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
                if let Some((gossip_peers, flood_peers)) = self.topic_peers.get_mut(&topic) {
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
                            if let Some(pos) = flood_peers.iter().position(|p| p == id) {
                                flood_peers.remove(pos);
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

    fn inject_node_event(&mut self, propagation_source: PeerId, event: InnerMessage) {
        // ignore successful sends event
        let event = match event {
            InnerMessage::Rx(event) => event,
            InnerMessage::Sent => return,
        };
        // Handle subscriptions
        // Update connected peers topics
        self.handle_received_subscriptions(&event.subscriptions, &propagation_source);

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

        loop {
            match self.heartbeat.poll() {
                // heartbeat ready
                Ok(Async::Ready(Some(_))) => self.heartbeat(),
                _ => break,
            };
        }

        Async::NotReady
    }
}

/// Transmission between the `OneShotHandler` and the `GossipsubRpc`.
pub enum InnerMessage {
    /// We received an RPC from a remote.
    Rx(GossipsubRpc),
    /// We successfully sent an RPC request.
    Sent,
}

impl From<GossipsubRpc> for InnerMessage {
    #[inline]
    fn from(rpc: GossipsubRpc) -> InnerMessage {
        InnerMessage::Rx(rpc)
    }
}

impl From<()> for InnerMessage {
    #[inline]
    fn from(_: ()) -> InnerMessage {
        InnerMessage::Sent
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
    use libp2p_core::topology::MemoryTopology;
    use libp2p_floodsub::TopicBuilder;

    // helper functions for testing

    // This function generates `peer_no` random PeerId's, subscribes to `topics` and subscribes the
    // injected nodes to all topics if `to_subscribe` is set. All nodes are considered gossipsub nodes.
    fn build_and_inject_nodes(
        peer_no: usize,
        topics: Vec<String>,
        to_subscribe: bool,
    ) -> (
        Gossipsub<tokio::net::TcpStream>,
        Vec<PeerId>,
        Vec<TopicHash>,
    ) {
        // generate a default GossipsubConfig
        let gs_config = GossipsubConfig::default();
        // create a gossipsub struct
        let mut gs: Gossipsub<tokio::net::TcpStream> = Gossipsub::new(PeerId::random(), gs_config);

        let mut topic_hashes = vec![];

        // subscribe to the topics
        for t in topics {
            let topic = TopicBuilder::new(t).build();
            gs.subscribe(topic.clone());
            topic_hashes.push(topic.hash().clone());
        }

        // build and connect peer_no random peers
        let mut peers = vec![];
        let dummy_connected_point = ConnectedPoint::Dialer {
            address: "/ip4/0.0.0.0/tcp/0".parse().unwrap(),
        };

        for _ in 0..peer_no {
            let peer = PeerId::random();
            peers.push(peer.clone());
            <Gossipsub<tokio::net::TcpStream> as NetworkBehaviour<MemoryTopology>>::inject_connected(&mut gs,
                peer.clone(),
                dummy_connected_point.clone(),
            );
            if to_subscribe {
                gs.handle_received_subscriptions(
                    &topic_hashes
                        .iter()
                        .cloned()
                        .map(|t| GossipsubSubscription {
                            action: GossipsubSubscriptionAction::Subscribe,
                            topic_hash: t,
                        })
                        .collect(),
                    &peer,
                );
            };
        }

        return (gs, peers, topic_hashes);
    }

    #[test]
    /// Test local node subscribing to a topic
    fn test_subscribe() {
        // The node should:
        // - Create an empty vector in mesh[topic]
        // - Send subscription request to all peers
        // - run JOIN(topic)

        let subscribe_topic = vec![String::from("test_subscribe")];
        let (gs, _, topic_hashes) = build_and_inject_nodes(20, subscribe_topic, true);

        assert!(
            *gs.mesh.get(&topic_hashes[0]).expect("Mesh should exist") == Vec::<PeerId>::new(),
            "Subscribe should add a new entry to the mesh[topic] hashmap"
        );

        // collect all the subscriptions
        let subscriptions =
            gs.events
                .iter()
                .fold(vec![], |mut collected_subscriptions, e| match e {
                    NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                        for s in &event.subscriptions {
                            match s.action {
                                GossipsubSubscriptionAction::Subscribe => {
                                    collected_subscriptions.push(s.clone())
                                }
                                _ => {}
                            };
                        }
                        collected_subscriptions
                    }
                    _ => collected_subscriptions,
                });

        // we sent a subscribe to all known peers
        assert!(
            subscriptions.len() == 20,
            "Should send a subscription to all known peers"
        );
    }

    #[test]
    /// Test unsubscribe.
    fn test_unsubscribe() {
        // Unsubscribe should:
        // - Remove the mesh entry for topic
        // - Send UNSUBSCRIBE to all known peers
        // - Call Leave

        let topic_strings = vec![String::from("topic1"), String::from("topic2")];
        let topics = topic_strings
            .iter()
            .map(|t| TopicBuilder::new(t.clone()).build())
            .collect::<Vec<Topic>>();

        // subscribe to topic_strings
        let (mut gs, _, topic_hashes) = build_and_inject_nodes(20, topic_strings, true);

        for topic_hash in &topic_hashes {
            assert!(
                gs.topic_peers.get(&topic_hash).is_some(),
                "Topic_peers contain a topic entry"
            );
            assert!(
                gs.mesh.get(&topic_hash).is_some(),
                "mesh should contain a topic entry"
            );
        }

        // unsubscribe from both topics
        assert!(
            gs.unsubscribe(topics[0].clone()),
            "should be able to unsubscribe successfully from each topic",
        );
        assert!(
            gs.unsubscribe(topics[1].clone()),
            "should be able to unsubscribe successfully from each topic",
        );

        let subscriptions =
            gs.events
                .iter()
                .fold(vec![], |mut collected_subscriptions, e| match e {
                    NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                        for s in &event.subscriptions {
                            match s.action {
                                GossipsubSubscriptionAction::Unsubscribe => {
                                    collected_subscriptions.push(s.clone())
                                }
                                _ => {}
                            };
                        }
                        collected_subscriptions
                    }
                    _ => collected_subscriptions,
                });

        // we sent a unsubscribe to all known peers, for two topics
        assert!(
            subscriptions.len() == 40,
            "Should send an unsubscribe event to all known peers"
        );

        // check we clean up internal structures
        for topic_hash in &topic_hashes {
            assert!(
                gs.mesh.get(&topic_hash).is_none(),
                "All topics should have been removed from the mesh"
            );
        }
    }

    #[test]
    /// Test JOIN(topic) functionality.
    fn test_join() {
        // The Join function should:
        // - Remove peers from fanout[topic]
        // - Add any fanout[topic] peers to the mesh (up to mesh_n)
        // - Fill up to mesh_n peers from known gossipsub peers in the topic
        // - Send GRAFT messages to all nodes added to the mesh

        // This test is not an isolated unit test, rather it uses higher level,
        // subscribe/unsubscribe to perform the test.

        let topic_strings = vec![String::from("topic1"), String::from("topic2")];
        let topics = topic_strings
            .iter()
            .map(|t| TopicBuilder::new(t.clone()).build())
            .collect::<Vec<Topic>>();

        let (mut gs, _, topic_hashes) = build_and_inject_nodes(20, topic_strings, true);

        // unsubscribe, then call join to invoke functionality
        assert!(
            gs.unsubscribe(topics[0].clone()),
            "should be able to unsubscribe successfully"
        );
        assert!(
            gs.unsubscribe(topics[1].clone()),
            "should be able to unsubscribe successfully"
        );

        // re-subscribe - there should be peers associated with the topic
        assert!(
            gs.subscribe(topics[0].clone()),
            "should be able to subscribe successfully"
        );

        // should have added mesh_n nodes to the mesh
        assert!(
            gs.mesh.get(&topic_hashes[0]).unwrap().len() == 6,
            "Should have added 6 nodes to the mesh"
        );

        // there should be mesh_n GRAFT messages.
        let graft_messages = gs
            .events
            .iter()
            .fold(vec![], |mut collected_grafts, e| match e {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    for c in &event.control_msgs {
                        match c {
                            GossipsubControlAction::Graft { topic_hash: _ } => {
                                collected_grafts.push(c.clone())
                            }
                            _ => {}
                        };
                    }
                    collected_grafts
                }
                _ => collected_grafts,
            });

        assert!(
            graft_messages.len() == 6,
            "There should be 6 grafts messages sent to peers"
        );

        // verify fanout nodes
        // add 3 random peers to the fanout[topic1]
        gs.fanout.insert(topic_hashes[1].clone(), vec![]);
        let new_peers = vec![];
        for _ in 0..3 {
            let mut fanout_peers = gs.fanout.get_mut(&topic_hashes[1]).unwrap();
            fanout_peers.push(PeerId::random());
        }

        // subscribe to topic1
        gs.subscribe(topics[1].clone());

        // the three new peers should have been added, along with 3 more from the pool.
        assert!(
            gs.mesh.get(&topic_hashes[1]).unwrap().len() == 6,
            "Should have added 6 nodes to the mesh"
        );
        let mesh_peers = gs.mesh.get(&topic_hashes[1]).unwrap();
        for new_peer in new_peers {
            assert!(
                mesh_peers.contains(new_peer),
                "Fanout peer should be included in the mesh"
            );
        }

        // there should now be 12 graft messages to be sent
        let graft_messages = gs
            .events
            .iter()
            .fold(vec![], |mut collected_grafts, e| match e {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    for c in &event.control_msgs {
                        match c {
                            GossipsubControlAction::Graft { topic_hash: _ } => {
                                collected_grafts.push(c.clone())
                            }
                            _ => {}
                        };
                    }
                    collected_grafts
                }
                _ => collected_grafts,
            });

        assert!(
            graft_messages.len() == 12,
            "There should be 6 grafts messages sent to peers"
        );
    }

    #[test]
    /// Test the gossipsub NetworkBehaviour peer connection logic.
    fn test_inject_connected() {
        let (gs, peers, topic_hashes) = build_and_inject_nodes(
            20,
            vec![String::from("topic1"), String::from("topic2")],
            true,
        );

        // check that our subscriptions are sent to each of the peers
        // collect all the SendEvents
        let send_events: Vec<&NetworkBehaviourAction<GossipsubRpc, GossipsubEvent>> = gs
            .events
            .iter()
            .filter(|e| match e {
                NetworkBehaviourAction::SendEvent {
                    peer_id: _,
                    event: _,
                } => true,
                _ => false,
            })
            .collect();

        // check that there are two subscriptions sent to each peer
        for sevent in send_events.clone() {
            match sevent {
                NetworkBehaviourAction::SendEvent { peer_id: _, event } => {
                    assert!(
                        event.subscriptions.len() == 2,
                        "There should be two subscriptions sent to each peer (1 for each topic)."
                    );
                }
                _ => {}
            };
        }

        // check that there are 20 send events created
        assert!(
            send_events.len() == 20,
            "There should be a subscription event sent to each peer."
        );

        // should add the new peers to `peer_topics` with an empty vec as a gossipsub node
        for peer in peers {
            let known_topics = &gs.peer_topics.get(&peer).unwrap().0;
            let node_type = &gs.peer_topics.get(&peer).unwrap().1;
            assert!(
                known_topics == &SmallVec::<[TopicHash; 16]>::from_vec(topic_hashes.clone()),
                "The topics for each node should all topics"
            );
            // TODO: Update this for handling floodsub nodes
            assert!(
                match node_type {
                    NodeType::Gossipsub => true,
                    _ => false,
                },
                "All peers should be added as a gossipsub node"
            );
        }
    }

    #[test]
    /// Test subscription handling
    fn test_handle_received_subscriptions() {
        // For every subscription:
        // SUBSCRIBE:   - Add subscribed topic to peer_topics for peer.
        //              - Add peer to topics_peer.
        // UNSUBSCRIBE  - Remove topic from peer_topics for peer.
        //              - Remove peer from topic_peers.

        let topics = vec!["topic1", "topic2", "topic3", "topic4"]
            .iter()
            .map(|&t| String::from(t))
            .collect();
        let (mut gs, peers, topic_hashes) = build_and_inject_nodes(20, topics, false);

        // The first peer sends 3 subscriptions and 1 unsubscription
        let mut subscriptions = topic_hashes[..3]
            .iter()
            .map(|topic_hash| GossipsubSubscription {
                action: GossipsubSubscriptionAction::Subscribe,
                topic_hash: topic_hash.clone(),
            })
            .collect::<Vec<GossipsubSubscription>>();

        subscriptions.push(GossipsubSubscription {
            action: GossipsubSubscriptionAction::Unsubscribe,
            topic_hash: topic_hashes[topic_hashes.len() - 1].clone(),
        });

        let unknown_peer = PeerId::random();
        // process the subscriptions
        // first and second peers send subscriptions
        gs.handle_received_subscriptions(&subscriptions, &peers[0]);
        gs.handle_received_subscriptions(&subscriptions, &peers[1]);
        // unknown peer sends the same subscriptions
        gs.handle_received_subscriptions(&subscriptions, &unknown_peer);

        // verify the result

        let peer_topics = gs.peer_topics.get(&peers[0]).unwrap().0.clone();
        assert!(
            peer_topics == SmallVec::<[TopicHash; 16]>::from_vec(topic_hashes[..3].to_vec()),
            "First peer should be subscribed to three topics"
        );
        let peer_topics = gs.peer_topics.get(&peers[1]).unwrap().0.clone();
        assert!(
            peer_topics == SmallVec::<[TopicHash; 16]>::from_vec(topic_hashes[..3].to_vec()),
            "Second peer should be subscribed to three topics"
        );

        assert!(
            gs.peer_topics.get(&unknown_peer).is_none(),
            "Unknown peer should not have been added"
        );

        for topic_hash in topic_hashes[..3].iter() {
            let topic_peers = gs.topic_peers.get(topic_hash).unwrap().0.clone(); // only gossipsub at the moment
            assert!(
                topic_peers == peers[..2].to_vec(),
                "Two peers should be added to the first three topics"
            );
        }

        // Peer 0 unsubscribes from the first topic

        gs.handle_received_subscriptions(
            &vec![GossipsubSubscription {
                action: GossipsubSubscriptionAction::Unsubscribe,
                topic_hash: topic_hashes[0].clone(),
            }],
            &peers[0],
        );

        let peer_topics = gs.peer_topics.get(&peers[0]).unwrap().0.clone();
        assert!(
            peer_topics == SmallVec::<[TopicHash; 16]>::from_vec(topic_hashes[1..3].to_vec()),
            "Peer should be subscribed to two topics"
        );

        let topic_peers = gs.topic_peers.get(&topic_hashes[0]).unwrap().0.clone(); // only gossipsub at the moment
        assert!(
            topic_peers == peers[1..2].to_vec(),
            "Only the second peers should be in the first topic"
        );
    }

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
        // test the filter
        let random_peers = gs.get_random_peers(&topic_hash, 5, { |_| false });
        assert!(random_peers.len() == 0, "Expected 0 peers to be returned");
        let random_peers = gs.get_random_peers(&topic_hash, 10, { |peer| peers.contains(peer) });
        assert!(random_peers.len() == 10, "Expected 10 peers to be returned");
    }

}
