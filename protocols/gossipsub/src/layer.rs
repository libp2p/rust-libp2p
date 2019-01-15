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
use protocol::{
    GossipsubMessage, GossipsubRpc, GossipsubSubscription, GossipsubSubscriptionAction,
};
use rand;
use smallvec::SmallVec;
use std::collections::hash_map::{DefaultHasher, HashMap};
use std::{collections::VecDeque, iter, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct Gossipsub<TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<NetworkBehaviourAction<GossipsubRpc, GossipsubEvent>>,

    /// Peer id of the local node. Used for the source of the messages that we publish.
    local_peer_id: PeerId,

    /// List of peers the network is connected to, and the topics that they're subscribed to.
    // TODO: filter out peers that don't support gossipsub so that we avoid hammering them with
    //       opened substream
    connected_peers: HashMap<PeerId, SmallVec<[TopicHash; 8]>>,

    // List of topics we're subscribed to. Necessary to filter out messages that we receive
    // erroneously.
    subscribed_topics: SmallVec<[Topic; 16]>,

    // We keep track of the messages we received (in the format `hash(source ID, seq_no)`) so that
    // we don't dispatch the same message twice if we receive it twice on the network.
    received: CuckooFilter<DefaultHasher>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
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
            marker: PhantomData,
        }
    }

    /// Subscribes to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        if self
            .subscribed_topics
            .iter()
            .any(|t| t.hash() == topic.hash())
        {
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
        let pos = match self
            .subscribed_topics
            .iter()
            .position(|t| t.hash() == topic)
        {
            Some(pos) => pos,
            None => return false,
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

        // Don't publish the message if we're not subscribed ourselves to any of the topics.
        if !self
            .subscribed_topics
            .iter()
            .any(|t| message.topics.iter().any(|u| t.hash() == u))
        {
            return;
        }

        self.received.add(&message);

        // Send to peers we know are subscribed to the topic.
        for (peer_id, sub_topic) in self.connected_peers.iter() {
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

        self.connected_peers.insert(id.clone(), SmallVec::new());
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        let was_in = self.connected_peers.remove(id);
        debug_assert!(was_in.is_some());
    }

    fn inject_node_event(&mut self, propagation_source: PeerId, event: GossipsubRpc) {
        // Update connected peers topics
        for subscription in event.subscriptions {
            let mut remote_peer_topics = self.connected_peers
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
            for (peer_id, subscr_topics) in self.connected_peers.iter() {
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
