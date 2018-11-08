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

use fnv::{FnvHashSet, FnvHasher};
use futures::prelude::*;
use handler::FloodsubHandler;
use libp2p_core::nodes::{ConnectedPoint, NetworkBehavior, NetworkBehaviorAction};
use libp2p_core::{nodes::protocols_handler::ProtocolsHandler, PeerId};
use protocol::{FloodsubMessage, FloodsubRpc};
use smallvec::SmallVec;
use std::{collections::HashSet, collections::VecDeque, hash::Hash, hash::Hasher, iter, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};
use topic::{Topic, TopicHash};

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct FloodsubBehaviour<TSubstream> {
    /// Events that need to be produced outside when polling.
    events: VecDeque<NetworkBehaviorAction<FloodsubRpc, FloodsubMessage>>,

    /// Peer id of the local node. Used for the source of the messages that we publish.
    local_peer_id: PeerId,

    /// List of peers the network is connected to.
    connected_peers: HashSet<PeerId>,

    // List of topics we're subscribed to. Necessary in order to filter out messages that we
    // erroneously receive.
    subscribed_topics: SmallVec<[Topic; 16]>,

    // Sequence number for the messages we send.
    seq_no: usize,

    // We keep track of the messages we received (in the format `hash(source ID, seq_no)`) so that
    // we don't dispatch the same message twice if we receive it twice on the network.
    // TODO: the `HashSet` will keep growing indefinitely :-/
    received: FnvHashSet<u64>,

    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> FloodsubBehaviour<TSubstream> {
    /// Creates a `FloodsubBehaviour`.
    pub fn new(local_peer_id: PeerId) -> Self {
        FloodsubBehaviour {
            events: VecDeque::new(),
            local_peer_id,
            connected_peers: HashSet::new(),
            subscribed_topics: SmallVec::new(),
            seq_no: 0,
            received: FnvHashSet::default(),
            marker: PhantomData,
        }
    }
}

impl<TSubstream> FloodsubBehaviour<TSubstream> {
    /// Subscribes to a topic.
    ///
    /// Returns true if the subscription worked. Returns false if we were already subscribed.
    pub fn subscribe(&mut self, topic: Topic) -> bool {
        if self.subscribed_topics.iter().any(|t| t.hash() == topic.hash()) {
            false
        } else {
            self.subscribed_topics.push(topic);
            true
        }
    }

    /// Unsubscribes from a topic.
    ///
    /// Returns true if we were subscribed to this topic.
    pub fn unsubscribe(&mut self, topic: impl AsRef<TopicHash>) -> bool {
        let topic = topic.as_ref();
        let pos = self.subscribed_topics.iter().position(|t| t.hash() == topic);
        if let Some(pos) = pos {
            self.subscribed_topics.remove(pos);
            true
        } else {
            false
        }
    }

    /// Publishes a message to the network.
    pub fn publish(&mut self, topic: impl Into<TopicHash>, data: impl Into<Vec<u8>>) {
        self.publish_many(iter::once(topic), data)
    }

    /// Publishes a message to the network that has multiple topics.
    pub fn publish_many(&mut self, topic: impl IntoIterator<Item = impl Into<TopicHash>>, data: impl Into<Vec<u8>>) {
        let message = FloodsubMessage {
            source: self.local_peer_id.clone(),
            data: data.into(),
            sequence_number: self.next_sequence_number(),
            topics: topic.into_iter().map(|t| t.into().clone()).collect(),
        };

        self.send_rpc(FloodsubRpc {
            subscriptions: Vec::new(),
            messages: vec![message],
        });
    }

    /// Builds a unique sequence number to put in a `FloodsubMessage`.
    fn next_sequence_number(&mut self) -> Vec<u8> {
        let data = self.seq_no.to_string();
        self.seq_no += 1;
        data.into()
    }

    /// Internal function that handles transmitting messages to all the peers we're connected to.
    fn send_rpc(&mut self, rpc: FloodsubRpc) {
        for message in &rpc.messages {
            self.received.insert(hash(message));
        }

        if rpc.messages.is_empty() && rpc.subscriptions.is_empty() {
            return;
        }

        for peer in self.connected_peers.iter() {
            self.events.push_back(NetworkBehaviorAction::SendEvent {
                peer_id: peer.clone(),
                event: rpc.clone(),
            });
        }
    }
}

impl<TSubstream> NetworkBehavior for FloodsubBehaviour<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static,
{
    type ProtocolsHandler = FloodsubHandler<TSubstream>;
    type OutEvent = FloodsubMessage;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        FloodsubHandler::new()
    }

    fn inject_connected(&mut self, id: PeerId, _: ConnectedPoint) {
        self.connected_peers.insert(id);
    }

    fn inject_disconnected(&mut self, id: &PeerId, _: ConnectedPoint) {
        let was_in = self.connected_peers.remove(id);
        debug_assert!(was_in);
    }

    fn inject_node_event(
        &mut self,
        _: PeerId,
        event: FloodsubRpc,
    ) {
        let mut rpc_to_dispatch = FloodsubRpc {
            messages: Vec::with_capacity(event.messages.len()),
            subscriptions: Vec::new(),
        };

        for message in event.messages {
            // Use `self.received` to store the messages that we have already received in the past.
            let message_hash = hash(&message);
            if !self.received.insert(message_hash) {
                continue;
            }

            // Add the message to be dispatched outside of the layer.
            if self.subscribed_topics.iter().any(|t| message.topics.iter().any(|u| t.hash() == u)) {
                self.events.push_back(NetworkBehaviorAction::GenerateEvent(message.clone()));
            }

            // Propagate the message to everyone else.
            rpc_to_dispatch.messages.push(message);
        }

        self.send_rpc(rpc_to_dispatch);
    }

    fn poll(
        &mut self,
    ) -> Async<
        NetworkBehaviorAction<
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

// Shortcut function that hashes a message.
#[inline]
fn hash(message: &FloodsubMessage) -> u64 {
    let mut h = FnvHasher::default();
    (&message.source, &message.sequence_number).hash(&mut h);
    h.finish()
}
