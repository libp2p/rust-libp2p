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
use handler::FloodsubHandler;
use libp2p_core::swarm::{NetworkBehaviour, PollParameters, SwarmEvent};
use libp2p_core::{protocols_handler::ProtocolsHandler, PeerId};
use protocol::{FloodsubMessage, FloodsubRpc, FloodsubSubscription, FloodsubSubscriptionAction};
use rand;
use smallvec::SmallVec;
use std::{collections::VecDeque, iter, marker::PhantomData};
use std::collections::hash_map::{DefaultHasher, HashMap};
use tokio_io::{AsyncRead, AsyncWrite};
use topic::{Topic, TopicHash};

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct Floodsub<TSubstream> {
    /// Events that need to be yielded to the outside when polling.
    events: VecDeque<FloodsubEvent>,

    /// Events to send to the protocols handler.
    to_send: VecDeque<(PeerId, FloodsubRpc)>,

    /// Peer id of the local node. Used for the source of the messages that we publish.
    local_peer_id: PeerId,

    /// List of peers the network is connected to, and the topics that they're subscribed to.
    // TODO: filter out peers that don't support floodsub, so that we avoid hammering them with
    //       opened substreams
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

impl<TSubstream> Floodsub<TSubstream> {
    /// Creates a `Floodsub`.
    pub fn new(local_peer_id: PeerId) -> Self {
        Floodsub {
            events: VecDeque::new(),
            to_send: VecDeque::new(),
            local_peer_id,
            connected_peers: HashMap::new(),
            subscribed_topics: SmallVec::new(),
            received: CuckooFilter::new(),
            marker: PhantomData,
        }
    }
}

impl<TSubstream> Floodsub<TSubstream> {
    /// Subscribes to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        if self.subscribed_topics.iter().any(|t| t.hash() == topic.hash()) {
            return false;
        }

        for peer in self.connected_peers.keys() {
            self.to_send.push_back((peer.clone(), FloodsubRpc {
                messages: Vec::new(),
                subscriptions: vec![FloodsubSubscription {
                    topic: topic.hash().clone(),
                    action: FloodsubSubscriptionAction::Subscribe,
                }],
            }));
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
        let pos = match self.subscribed_topics.iter().position(|t| t.hash() == topic) {
            Some(pos) => pos,
            None => return false
        };

        self.subscribed_topics.remove(pos);

        for peer in self.connected_peers.keys() {
            self.to_send.push_back((peer.clone(), FloodsubRpc {
                messages: Vec::new(),
                subscriptions: vec![FloodsubSubscription {
                    topic: topic.clone(),
                    action: FloodsubSubscriptionAction::Unsubscribe,
                }]
            }));
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
    pub fn publish_many(&mut self, topic: impl IntoIterator<Item = impl Into<TopicHash>>, data: impl Into<Vec<u8>>) {
        let message = FloodsubMessage {
            source: self.local_peer_id.clone(),
            data: data.into(),
            // If the sequence numbers are predictable, then an attacker could flood the network
            // with packets with the predetermined sequence numbers and absorb our legitimate
            // messages. We therefore use a random number.
            sequence_number: rand::random::<[u8; 20]>().to_vec(),
            topics: topic.into_iter().map(|t| t.into().clone()).collect(),
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

            self.to_send.push_back((peer_id.clone(), FloodsubRpc {
                subscriptions: Vec::new(),
                messages: vec![message.clone()],
            }));
        }
    }

    fn process_node_event(
        &mut self,
        propagation_source: PeerId,
        event: FloodsubRpc,
    ) {
        // Update connected peers topics
        for subscription in event.subscriptions {
            let mut remote_peer_topics = self.connected_peers
                .get_mut(&propagation_source)
                .expect("connected_peers is kept in sync with the peers we are connected to; we are guaranteed to only receive events from connected peers; QED");
            match subscription.action {
                FloodsubSubscriptionAction::Subscribe => {
                    if !remote_peer_topics.contains(&subscription.topic) {
                        remote_peer_topics.push(subscription.topic.clone());
                    }
                    self.events.push_back(FloodsubEvent::Subscribed {
                        peer_id: propagation_source.clone(),
                        topic: subscription.topic,
                    });
                }
                FloodsubSubscriptionAction::Unsubscribe => {
                    if let Some(pos) = remote_peer_topics.iter().position(|t| t == &subscription.topic ) {
                        remote_peer_topics.remove(pos);
                    }
                    self.events.push_back(FloodsubEvent::Unsubscribed {
                        peer_id: propagation_source.clone(),
                        topic: subscription.topic,
                    });
                }
            }
        }

        // List of messages we're going to propagate on the network.
        let mut rpcs_to_dispatch: Vec<(PeerId, FloodsubRpc)> = Vec::new();

        for message in event.messages {
            // Use `self.received` to skip the messages that we have already received in the past.
            // Note that this can false positive.
            if !self.received.test_and_add(&message) {
                continue;
            }

            // Add the message to be dispatched to the user.
            if self.subscribed_topics.iter().any(|t| message.topics.iter().any(|u| t.hash() == u)) {
                let event = FloodsubEvent::Message(message.clone());
                self.events.push_back(event);
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
                    rpcs_to_dispatch.push((peer_id.clone(), FloodsubRpc {
                        subscriptions: Vec::new(),
                        messages: vec![message.clone()],
                    }));
                }
            }
        }

        for (peer_id, rpc) in rpcs_to_dispatch {
            self.to_send.push_back((peer_id, rpc));
        }
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for Floodsub<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = FloodsubHandler<TSubstream>;
    type OutEvent = FloodsubEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        FloodsubHandler::new()
    }

    fn poll(
        &mut self,
        event: SwarmEvent<FloodsubRpc>,
        params: &mut PollParameters<TTopology, FloodsubRpc>,
    ) -> Async<FloodsubEvent> {
        if let Some((peer_id, event)) = self.to_send.pop_front() {
            params.send_event(peer_id, event);
        }

        match event {
            SwarmEvent::Connected { peer_id, .. } => {
                // We need to send our subscriptions to the newly-connected node.
                for topic in self.subscribed_topics.iter() {
                    params.send_event(peer_id.clone(), FloodsubRpc {
                        messages: Vec::new(),
                        subscriptions: vec![FloodsubSubscription {
                            topic: topic.hash().clone(),
                            action: FloodsubSubscriptionAction::Subscribe,
                        }],
                    });
                }

                self.connected_peers.insert(peer_id.clone(), SmallVec::new());
            }
            SwarmEvent::Disconnected { peer_id, .. } => {
                let was_in = self.connected_peers.remove(peer_id);
                debug_assert!(was_in.is_some());
            }
            SwarmEvent::ProtocolsHandlerEvent { peer_id, event } => {
                self.process_node_event(peer_id, event);
            },
            SwarmEvent::None => (),
        }

        if let Some(event) = self.events.pop_front() {
            return Async::Ready(event);
        }

        Async::NotReady
    }
}

/// Event that can happen on the floodsub behaviour.
#[derive(Debug)]
pub enum FloodsubEvent {
    /// A message has been received.
    Message(FloodsubMessage),

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
