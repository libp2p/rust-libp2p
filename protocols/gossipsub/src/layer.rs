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
            heartbeat_initial_delay: Duration::from_millis(100),
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
            heartbeat_initial_delay,
            heartbeat_interval,
            fanout_ttl,
        }
    }
}

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct Gossipsub<TSubstream> {
    /// Configuration providing gossipsub performance parameters
    config: GossipsubConfig,

    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, GossipsubEvent>>,

    /// Peer id of the local node. Used for the source of the messages that we publish.
    local_peer_id: PeerId,

    // These data structures may be combined in later revisions - kept for ease of iteration
    /// List of peers the network is connected to, and the topics that they're subscribed to.
    peer_topics: HashMap<PeerId, SmallVec<[TopicHash; 8]>>,
    /// Inverse hashmap of connected_peers - maps a topic to a tuple which contains a list of
    /// gossipsub peers and floodsub peers. Used to efficiently look up peers per topic.
    topic_peers: HashMap<TopicHash, (Vec<PeerId>, Vec<PeerId>)>,

    /* use topic_peers instead of two hashmaps
    /// Map of topics to connected gossipsub peers
    gossipsub_peers: HashMap<TopicHash, Vec<PeerId>>,
    /// Map of topics to connected floodsub peers
    floodsub_peers: HashMap<TopicHash, Vec<PeerId>>,
    */
    /// Overlay network of connected peers - Maps topics to connected gossipsub peers
    mesh: HashMap<TopicHash, Vec<PeerId>>,

    /// Map of topics to list of peers that we publish to, but don't subscribe to
    fanout: HashMap<TopicHash, Vec<PeerId>>,

    /// The last publish time for fanout topics
    fanout_last_pub: HashMap<TopicHash, Instant>,

    /// Message cache for the last few heartbeats
    mcache: MessageCache,

    // We keep track of the messages we received (in the format `hash(source ID, seq_no)`) so that
    // we don't dispatch the same message twice if we receive it twice on the network.
    received: CuckooFilter<DefaultHasher>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Gossipsub<TSubstream> {
    /// Creates a `Gossipsub`.
    pub fn new(local_peer_id: PeerId, gs_config: GossipsubConfig) -> Self {
        Gossipsub {
            config: gs_config.clone(),
            events: VecDeque::new(),
            local_peer_id,
            peer_topics: HashMap::new(),
            topic_peers: HashMap::new(),
            mesh: HashMap::new(),
            fanout: HashMap::new(),
            fanout_last_pub: HashMap::new(),
            mcache: MessageCache::new(gs_config.history_gossip, gs_config.history_length),
            received: CuckooFilter::new(),
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
        // TODO: Consolidate hashmap of peers
        for peer in self.peer_topics.keys() {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic.hash().clone(),
                        action: GossipsubSubscriptionAction::Subscribe,
                    }],
                    control_msgs: Vec::new(),
                },
            });
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
        for peer in self.peer_topics.keys() {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic_hash.clone(),
                        action: GossipsubSubscriptionAction::Unsubscribe,
                    }],
                    control_msgs: Vec::new(),
                },
            });
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

        // build a list of peers to forward the message to
        let mut recipient_peers: Vec<PeerId> = vec![];

        for t in message.topics.iter() {
            // floodsub peers in the topic - add them to recipient_peers
            if let Some((_, floodsub_peers)) = self.topic_peers.get(t) {
                for peer_id in floodsub_peers {
                    if !recipient_peers.contains(peer_id) {
                        recipient_peers.push(peer_id.clone());
                    }
                }
            }

            // gossipsub peers in the mesh
            match self.mesh.get(t) {
                // we are in the mesh, add the gossip peers to recipient_peers
                Some(gossip_peers) => {
                    for peer_id in gossip_peers {
                        if !recipient_peers.contains(peer_id) {
                            recipient_peers.push(peer_id.clone());
                        }
                    }
                }
                // not in the mesh, use fanout peers
                None => {
                    if self.fanout.contains_key(t) {
                        // we have fanout peers. Add them to recipient_peers
                        if let Some(fanout_peers) = self.fanout.get(t) {
                            for peer_id in fanout_peers {
                                if !recipient_peers.contains(peer_id) {
                                    recipient_peers.push(peer_id.clone());
                                }
                            }
                        }
                    } else {
                        // TODO: Ensure fanout key never contains an empty set
                        // we have no fanout peers, select mesh_n of them and add them to the fanout
                        let mesh_n = self.config.mesh_n;
                        let new_peers = self.get_random_peers(t, mesh_n);
                        // add the new peers to the fanout and recipient peers
                        self.fanout.insert(t.clone(), new_peers.clone());
                        for peer_id in new_peers {
                            if !recipient_peers.contains(&peer_id) {
                                recipient_peers.push(peer_id.clone());
                            }
                        }
                    }
                    // we are publishing to fanout peers - update the time we published
                    self.fanout_last_pub.insert(t.clone(), Instant::now());
                }
            }
        }

        // add published message to our received cache
        // TODO: Add to memcache
        self.received.add(&message.msg_id());

        // Send to peers we know are subscribed to the topic.
        for peer_id in recipient_peers {
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
            peers = self.get_random_peers(topic_hash, mesh_n);
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
                        topic: topic_hash.clone(),
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
                            topic: topic_hash.clone(),
                        }],
                    },
                });
                //TODO: untag Peer
            }
        }
    }

    /// Handles an IHAVE control message. Checks our cache of messages. If the message is unknown,
    /// requests it with an IWANT control message.
    fn handle_ihave(&mut self, peer_id: PeerId, ihave_msgs: Vec<(TopicHash, Vec<String>)>) {
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
    fn handle_iwant(&mut self, peer_id: PeerId, iwant_msgs: Vec<String>) {
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
    fn handle_graft(&mut self, peer_id: PeerId, topics: Vec<TopicHash>) {
        let mut to_prune_topics = HashMap::new();
        for topic in topics {
            if let Some(peers) = self.mesh.get_mut(&topic) {
                // if we are subscribed, add peer to the mesh
                println!("GRAFT: Mesh link added from {:?}", peer_id);
                peers.push(peer_id.clone());
            //TODO: tagPeer
            } else {
                to_prune_topics.insert(topic.clone(), ());
            }
        }

        if !to_prune_topics.is_empty() {
            // build the prune messages to send
            let prune_messages = to_prune_topics
                .keys()
                .map(|topic| GossipsubControlAction::Prune {
                    topic: topic.clone(),
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
    fn handle_prune(&mut self, peer_id: PeerId, topics: Vec<TopicHash>) {
        for topic in topics {
            if let Some(peers) = self.mesh.get_mut(&topic) {
                // remove the peer if it exists in the mesh
                if let Some(pos) = peers.iter().position(|p| p == &peer_id) {
                    peers.remove(pos);
                    //TODO: untagPeer
                }
            }
        }
    }

    /// Helper function to get a set of `n` random gossipsub peers for a topic
    fn get_random_peers(&self, topic_hash: &TopicHash, n: usize) -> Vec<PeerId> {
        let mut gossip_peers = match self.topic_peers.get(topic_hash) {
            Some((gossip_peers, _)) => gossip_peers.clone(),
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
        for topic in self.mesh.keys() {
            //TODO: Build a list of subscriptions
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic.clone(),
                        action: GossipsubSubscriptionAction::Subscribe,
                    }],
                    control_msgs: Vec::new(),
                },
            });
        }

        // TODO: Handle the peer addition
        self.peer_topics.insert(id.clone(), SmallVec::new());
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        // TODO: Handle peer diconnection
        let was_in = self.peer_topics.remove(id);
        debug_assert!(was_in.is_some());
    }

    fn inject_node_event(&mut self, propagation_source: PeerId, event: GossipsubRpc) {
        // Update connected peers topics
        for subscription in event.subscriptions {
            let mut remote_peer_topics = self.peer_topics
                .get_mut(&propagation_source)
                .expect("connected_peers is kept in sync with the peers we are connected to; we are guaranteed to only receive events from connected peers; QED");
            match subscription.action {
                GossipsubSubscriptionAction::Subscribe => {
                    if !remote_peer_topics.contains(&subscription.topic) {
                        remote_peer_topics.push(subscription.topic.clone());
                    }
                    // generates a subscription event to be polled
                    self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                        GossipsubEvent::Subscribed {
                            peer_id: propagation_source.clone(),
                            topic: subscription.topic,
                        },
                    ));
                }
                GossipsubSubscriptionAction::Unsubscribe => {
                    if let Some(pos) = remote_peer_topics
                        .iter()
                        .position(|t| t == &subscription.topic)
                    {
                        remote_peer_topics.remove(pos);
                    }
                    self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                        GossipsubEvent::Unsubscribed {
                            peer_id: propagation_source.clone(),
                            topic: subscription.topic,
                        },
                    ));
                }
            }
        }

        // List of messages we're going to propagate on the network.
        let mut rpcs_to_dispatch: Vec<(PeerId, GossipsubRpc)> = Vec::new();

        //TODO: Update for gossipsub
        for message in event.messages {
            // Use `self.received` to skip the messages that we have already received in the past.
            // Note that this can be a false positive.
            if !self.received.test_and_add(&message) {
                continue;
            }

            // Add the message to be dispatched to the user.
            /*
            if self
                .subscribed_topics
                .iter()
                .any(|t| message.topics.iter().any(|u| t.hash() == u))
            {
                let event = GossipsubEvent::Message(message.clone());
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(event));
            }
            */

            //TODO: Update for gossipsub
            // Propagate the message to everyone else who is subscribed to any of the topics.
            for (peer_id, subscr_topics) in self.peer_topics.iter() {
                if peer_id == &propagation_source {
                    continue;
                }

                if !subscr_topics
                    .iter()
                    .any(|t| message.topics.iter().any(|u| t == u))
                {
                    continue;
                }

                if let Some(pos) = rpcs_to_dispatch.iter().position(|(p, _)| p == peer_id) {
                    rpcs_to_dispatch[pos].1.messages.push(message.clone());
                } else {
                    rpcs_to_dispatch.push((
                        peer_id.clone(),
                        GossipsubRpc {
                            subscriptions: Vec::new(),
                            messages: vec![message.clone()],
                            control_msgs: Vec::new(),
                        },
                    ));
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

        let random_peers = gs.get_random_peers(&topic_hash, 5);
        assert!(random_peers.len() == 5, "Expected 5 peers to be returned");
        let random_peers = gs.get_random_peers(&topic_hash, 30);
        assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
        assert!(random_peers == peers, "Expected no shuffling");
        let random_peers = gs.get_random_peers(&topic_hash, 20);
        assert!(random_peers.len() == 20, "Expected 20 peers to be returned");
        assert!(random_peers == peers, "Expected no shuffling");
        let random_peers = gs.get_random_peers(&topic_hash, 0);
        assert!(random_peers.len() == 0, "Expected 0 peers to be returned");
    }

}
