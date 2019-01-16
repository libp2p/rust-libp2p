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
    GossipsubMessage, GossipsubRpc, GossipsubSubscription, GossipsubSubscriptionAction,
};
use rand;
use smallvec::SmallVec;
use std::collections::hash_map::{DefaultHasher, HashMap};
use std::time::Duration;
use std::{collections::VecDeque, iter, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};

// potentially rename this struct - due to clashes
/// Configuration parameters that define the performance of the gossipsub network
pub struct GossipsubConfig {
    /// Overlay network parameters
    /// Number of heartbeats to keep in the memcache
    history_length: usize,
    /// Number of past heartbeats to gossip about
    history_gossip: usize,

    /// Target number of peers for the mesh network
    mesh_n: usize,
    /// Minimum number of peers in mesh network before adding more
    mesh_n_low: usize,
    /// Maximum number of peers in mesh network before removing some
    mesh_n_high: usize,

    /// Initial delay in each heartbeat
    heartbeat_initial_delay: Duration,
    /// Time between each heartbeat
    heartbeat_interval: Duration,
    /// Time to live for fanout peers
    fanout_ttl: Duration,
}

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct Gossipsub<TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, GossipsubEvent>>,

    /// Peer id of the local node. Used for the source of the messages that we publish.
    local_peer_id: PeerId,

    // These data structures may be combined in later revisions - kept for ease of iteration
    /// List of peers the network is connected to, and the topics that they're subscribed to.
    peers_topic: HashMap<PeerId, SmallVec<[TopicHash; 8]>>,
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

    /// Message cache for the last few heartbeats
    mcache: MessageCache,

    // We keep track of the messages we received (in the format `hash(source ID, seq_no)`) so that
    // we don't dispatch the same message twice if we receive it twice on the network.
    received: CuckooFilter<DefaultHasher>,

    subscribed_topics: SmallVec<[Topic; 16]>,
    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Gossipsub<TSubstream> {
    /// Creates a `Gossipsub`.
    pub fn new(local_peer_id: PeerId, gs_config: GossipsubConfig) -> Self {
        Gossipsub {
            events: VecDeque::new(),
            local_peer_id,
            peers_topic: HashMap::new(),
            topic_peers: HashMap::new(),
            mesh: HashMap::new(),
            fanout: HashMap::new(),
            mcache: MessageCache::new(gs_config.history_gossip, gs_config.history_length),
            received: CuckooFilter::new(),
            subscribed_topics: SmallVec::new(),
            marker: PhantomData,
        }
    }

    /// Subscribes to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        // TODO: Can simply check if topic is in the mesh
        if self
            .subscribed_topics
            .iter()
            .any(|t| t.hash() == topic.hash())
        {
            return false;
        }

        // send subscription request to all floodsub and gossipsub peers
        for peer in self.peers_topic.keys() {
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

        self.subscribed_topics.push(topic.clone());

        // call JOIN(topic)
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
        // TODO: Check the mesh if we are subscribed
        let pos = match self
            .subscribed_topics
            .iter()
            .position(|t| t.hash() == topic_hash)
        {
            Some(pos) => pos,
            None => return false,
        };

        self.subscribed_topics.remove(pos);

        // announce to all floodsub and gossipsub peers
        for peer in self.peers_topic.keys() {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic_hash.clone(),
                        action: GossipsubSubscriptionAction::Unsubscribe,
                    }],
                },
            });
        }

        // call LEAVE(topic)
        self.leave(&topic);

        true
    }

    //TODO: Update publish for gossipsub
    /// Publishes a message to the network.
    ///
    /// > **Note**: Doesn't do anything if we're not subscribed to the topic.
    pub fn publish(&mut self, topic: impl Into<TopicHash>, data: impl Into<Vec<u8>>) {
        self.publish_many(iter::once(topic), data)
    }

    /// Publishes a message with multiple topics to the network.
    ///
    /// > **Note**: Doesn't do anything if we're not subscribed to any of the topics.
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
            sequence_number: rand::random::<[u8; 20]>().to_vec(),
            topics: topic.into_iter().map(|t| t.into().clone()).collect(),
        };

        // If we are not subscribed to the topic, forward to fanout peers
        // TODO: Can check mesh
        if !self
            .subscribed_topics
            .iter()
            .any(|t| message.topics.iter().any(|u| t.hash() == u))
        {
            //TODO: Send to fanout peers if exist - add fanout logic
            // loop through topics etc
            return;
        }

        self.received.add(&message);

        // Send to peers we know are subscribed to the topic.
        for (peer_id, sub_topic) in self.peers_topic.iter() {
            if !sub_topic
                .iter()
                .any(|t| message.topics.iter().any(|u| t == u))
            {
                continue;
            }

            println!("peers subscribed? {:?}", peer_id);
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: peer_id.clone(),
                event: GossipsubRpc {
                    subscriptions: Vec::new(),
                    messages: vec![message.clone()],
                },
            });
        }
    }

    fn join(&mut self, topic: impl AsRef<TopicHash>) {}

    fn leave(&mut self, topic: impl AsRef<TopicHash>) {}
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
        for topic in self.subscribed_topics.iter() {
            self.events.push_back(NetworkBehaviourAction::SendEvent {
                peer_id: id.clone(),
                event: GossipsubRpc {
                    messages: Vec::new(),
                    subscriptions: vec![GossipsubSubscription {
                        topic: topic.hash().clone(),
                        action: GossipsubSubscriptionAction::Subscribe,
                    }],
                },
            });
        }

        self.peers_topic.insert(id.clone(), SmallVec::new());
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        let was_in = self.peers_topic.remove(id);
        debug_assert!(was_in.is_some());
    }

    fn inject_node_event(&mut self, propagation_source: PeerId, event: GossipsubRpc) {
        // Update connected peers topics
        for subscription in event.subscriptions {
            let mut remote_peer_topics = self.peers_topic
                .get_mut(&propagation_source)
                .expect("connected_peers is kept in sync with the peers we are connected to; we are guaranteed to only receive events from connected peers; QED");
            match subscription.action {
                GossipsubSubscriptionAction::Subscribe => {
                    if !remote_peer_topics.contains(&subscription.topic) {
                        remote_peer_topics.push(subscription.topic.clone());
                    }
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

        for message in event.messages {
            // Use `self.received` to skip the messages that we have already received in the past.
            // Note that this can be a false positive.
            if !self.received.test_and_add(&message) {
                continue;
            }

            // Add the message to be dispatched to the user.
            if self
                .subscribed_topics
                .iter()
                .any(|t| message.topics.iter().any(|u| t.hash() == u))
            {
                let event = GossipsubEvent::Message(message.clone());
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(event));
            }

            // Propagate the message to everyone else who is subscribed to any of the topics.
            for (peer_id, subscr_topics) in self.peers_topic.iter() {
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
