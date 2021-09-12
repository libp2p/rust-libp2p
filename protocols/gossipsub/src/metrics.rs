// Copyright 2020 Sigma Prime Pty Ltd.
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

//! A set of metrics used to help track and diagnose the network behaviour of the gossipsub
//! protocol.

pub mod topic_metrics;

use crate::topic::TopicHash;
use libp2p_core::PeerId;
use log::warn;
use std::collections::HashMap;

use self::topic_metrics::{SlotChurnMetric, SlotMessageMetric, SlotMetricCounts, TopicMetrics};

/// A collection of metrics used throughout the gossipsub behaviour.
pub struct InternalMetrics {
    /// Current metrics for all known mesh data. See [`TopicMetrics`] for further information.
    pub topic_metrics: HashMap<TopicHash, TopicMetrics>,
    /// The number of broken promises (this metric is indicative of nodes with invalid message-ids)
    pub broken_promises: usize,
    /// When the user validates a message, it tries to re propagate it to its mesh peers. If the
    /// message expires from the memcache before it can be validated, we count this a cache miss
    /// and it is an indicator that the memcache size should be increased.
    pub memcache_misses: usize,
    /// Keeps track of the number of messages we have received on topics we are not subscribed
    /// to.
    pub messages_received_on_invalid_topic: usize,
}

impl Default for InternalMetrics {
    fn default() -> Self {
        InternalMetrics {
            topic_metrics: HashMap::new(),
            broken_promises: 0,
            memcache_misses: 0,
            messages_received_on_invalid_topic: 0,
        }
    }
}

impl InternalMetrics {
    /// Returns the slot metrics for a given topic
    pub fn slot_metrics_for_topic(
        &self,
        topic: &TopicHash,
    ) -> Option<impl Iterator<Item = &SlotMetricCounts>> {
        Some(self.topic_metrics.get(topic)?.slot_metrics_iter())
    }

    /// Churns a slot in the topic_metrics. This assumes the peer is in the mesh.
    pub fn churn_slot(
        &mut self,
        topic: &TopicHash,
        peer_id: &PeerId,
        churn_reason: SlotChurnMetric,
    ) {
        match self.topic_metrics.get_mut(topic) {
            Some(slot_data) => slot_data.churn_slot(peer_id, churn_reason),
            None => {
                warn!(
                "metrics_event[{}]: [slot --] increment {} peer {} FAILURE [retrieving slot_data]",
                topic, <SlotChurnMetric as Into<&'static str>>::into(churn_reason), peer_id,
            )
            }
        }
    }

    /// Increment a MessageMetric in the topic_metrics for peer in topic.
    pub fn increment_message_metric(
        &mut self,
        topic: &TopicHash,
        peer: &PeerId,
        message_metric: SlotMessageMetric,
    ) {
        self.topic_metrics
            .entry(topic.clone())
            .or_insert_with(|| TopicMetrics::new(topic.clone()))
            .increment_message_metric(peer, message_metric);
    }

    /// Assign slots in topic to peers.
    pub fn assign_slots_to_peers<U>(&mut self, topic: &TopicHash, peer_list: U)
    where
        U: Iterator<Item = PeerId>,
    {
        self.topic_metrics
            .entry(topic.clone())
            .or_insert_with(|| TopicMetrics::new(topic.clone()))
            .assign_slots_to_peers(peer_list);
    }

    /// Assigns a slot in topic to the peer if the peer doesn't already have one.
    pub fn assign_slot_if_unassigned(&mut self, topic: &TopicHash, peer: &PeerId) {
        self.topic_metrics
            .entry(topic.clone())
            .or_insert_with(|| TopicMetrics::new(topic.clone()))
            .assign_slot_if_unassigned(*peer);
    }
}
