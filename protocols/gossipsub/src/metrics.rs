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
//!
//! Note that if metrics are enabled, we store a lot of detail for each metric. Specifically, each metric is stored
//! per "slot" of each mesh. This means each metric is counted for each peer whilst they are in the
//! mesh. The exposed open metric values typically aggregate these into a per
//! mesh metric. Users are able to fine-grain their access to the more detailed metrics via the
//! [`slot_metrics_for_topic`] function.

pub mod topic_metrics;

use crate::topic::TopicHash;
use libp2p_core::PeerId;
use std::collections::{HashMap, HashSet};
use topic_metrics::Slot;

// use open_metrics_client::encoding::text::Encode;
use open_metrics_client::metrics::counter::Counter;
use open_metrics_client::metrics::family::Family;
use open_metrics_client::metrics::gauge::Gauge;
use open_metrics_client::metrics::histogram::{linear_buckets, Histogram};
use open_metrics_client::registry::Registry;

use self::topic_metrics::{SlotChurnMetric, SlotMessageMetric, SlotMetricCounts, TopicMetrics};

/// This provides an upper bound to the number of mesh topics we create metrics for. It prevents
/// unbounded labels being created in the metrics.
const MESH_TOPIC_LIMIT: usize = 300;
/// A separate limit is used to keep track of non-mesh topics. Mesh topics are controlled by the
/// user via subscriptions whereas non-mesh topics are determined by users on the network.
/// This limit permits a fixed amount of topics to allow, in-addition to the mesh topics.
const NON_MESH_TOPIC_LIMIT: usize = 50;

/// A collection of metrics used throughout the Gossipsub behaviour.
pub struct InternalMetrics {
    /// Keeps track of which mesh topics have been added to metrics or not.
    added_mesh_topics: HashSet<TopicHash>,
    /// Keeps track of which non mesh topics have been added to metrics or not.
    added_non_mesh_topics: HashSet<TopicHash>,
    /// The current peers in each mesh.
    mesh_peers: Family<TopicHash, Gauge>,
    /// The scores for each peer in each mesh.
    mesh_score: Family<TopicHash, Histogram>,
    /// The average peer score for each mesh.
    mesh_avg_score: Family<TopicHash, Gauge>,
    /// The total number of messages received (after duplicate filter).
    mesh_message_rx_total: Family<TopicHash, Counter>,
    /// The total number of messages sent.
    mesh_message_tx_total: Family<TopicHash, Counter>,
    /// The number of messages received from non-mesh peers (after duplicate filter).
    mesh_messages_from_non_mesh_peers: Family<TopicHash, Counter>,
    /// The total number of duplicate messages filtered per mesh.
    mesh_duplicates_filtered: Family<TopicHash, Counter>,
    /// The total number of messages validated per mesh.
    mesh_messages_validated: Family<TopicHash, Counter>,
    /// The total number of messages rejected per mesh.
    mesh_messages_rejected: Family<TopicHash, Counter>,
    /// The total number of messages ignored per mesh.
    mesh_messages_ignored: Family<TopicHash, Counter>,
    /// The number of first message delivers per slot per mesh.
    mesh_first_message_deliveries_per_slot: Family<(TopicHash, Slot), Gauge>,
    /// The number of IWANT requests being sent per mesh topic.
    mesh_iwant_requests: Family<TopicHash, Counter>,
    /// Number of peers subscribed to each known topic.
    topic_peers: Family<TopicHash, Gauge>,
    /// Number of peers subscribed to each subscribed topic.
    subscribed_topic_peers: Family<TopicHash, Gauge>,
    /// The number of broken promises (this metric is indicative of nodes with invalid message-ids)
    broken_promises: Counter,
    /// Keeps track of the number of messages we have received on topics we are not subscribed
    /// to.
    invalid_topic_messages: Counter,
    /// When the user validates a message, it tries to re propagate it to its mesh peers. If the
    /// message expires from the memcache before it can be validated, we count this a cache miss
    /// and it is an indicator that the memcache size should be increased.
    memcache_misses: Counter,
    /// Current metrics for all known mesh data. See [`TopicMetrics`] for further information.
    topic_metrics: HashMap<TopicHash, TopicMetrics>,
}

pub struct Config {
    pub score_histogram_buckets: Vec<f64>,
}

impl Config {
    pub fn histogram(&self) -> Histogram {
        Histogram::new(self.score_histogram_buckets.clone().into_iter())
    }
}

impl InternalMetrics {
    /// Constructs and builds the internal metrics given a registry.
    pub fn new(registry: &mut Registry, _config: Config) -> Self {
        /* Mesh Metrics */

        let mesh_peers = Family::default();
        registry.register(
            "mesh_peer_count",
            "Number of peers in each mesh",
            Box::new(mesh_peers.clone()),
        );

        // TODO: change after https://github.com/mxinden/rust-open-metrics-client/pull/21 and use
        // for now the range -10K to 10K with 100 long intervals as reasonable default.
        // let mesh_score = Family::new_with_constructor(config);
        let mesh_score =
            Family::new_with_constructor(|| Histogram::new(linear_buckets(-10_000.0, 100.0, 201)));
        registry.register(
            "mesh_score",
            "Score of all peers in each mesh",
            Box::new(mesh_score.clone()),
        );

        let mesh_avg_score = Family::default();
        registry.register(
            "mesh_avg_score",
            "Average score of all peers in each mesh",
            Box::new(mesh_avg_score.clone()),
        );

        let mesh_message_rx_total = Family::default();
        registry.register(
            "mesh_message_rx_total",
            "Total number of messages received from each mesh",
            Box::new(mesh_message_rx_total.clone()),
        );

        let mesh_message_tx_total = Family::default();
        registry.register(
            "mesh_message_tx_total",
            "Total number of messages sent in each mesh",
            Box::new(mesh_message_tx_total.clone()),
        );

        let mesh_messages_from_non_mesh_peers = Family::default();
        registry.register(
            "messages_from_non_mesh_peers",
            "Number of messages received from peers not in the mesh, for each mesh",
            Box::new(mesh_messages_from_non_mesh_peers.clone()),
        );

        let mesh_duplicates_filtered = Family::default();
        registry.register(
            "mesh_duplicates_filtered",
            "Total number of duplicate messages filtered in each mesh",
            Box::new(mesh_duplicates_filtered.clone()),
        );

        let mesh_messages_validated = Family::default();
        registry.register(
            "mesh_messages_validated",
            "Total number of messages that have been validated in each mesh",
            Box::new(mesh_messages_validated.clone()),
        );

        let mesh_messages_rejected = Family::default();
        registry.register(
            "mesh_messages_rejected",
            "Total number of messages rejected in each mesh",
            Box::new(mesh_messages_rejected.clone()),
        );

        let mesh_messages_ignored = Family::default();
        registry.register(
            "mesh_messages_ignored",
            "Total number of messages ignored in each mesh",
            Box::new(mesh_messages_ignored.clone()),
        );

        let mesh_first_message_deliveries_per_slot = Family::default();
        registry.register(
            "mesh_first_message_deliveries_per_slot",
            "The number of first message deliveries per mesh slot",
            Box::new(mesh_first_message_deliveries_per_slot.clone()),
        );

        let mesh_iwant_requests = Family::default();
        registry.register(
            "mesh_iwant_requests",
            "The number of IWANT requests per mesh",
            Box::new(mesh_first_message_deliveries_per_slot.clone()),
        );

        let broken_promises = Counter::default();
        registry.register(
            "broken_promises",
            "Total number of broken promises per mesh",
            Box::new(broken_promises.clone()),
        );

        /* Peer Metrics */

        let topic_peers = Family::default();
        registry.register(
            "topic_peer_count",
            "Number of peers subscribed to each known topic",
            Box::new(topic_peers.clone()),
        );

        let subscribed_topic_peers = Family::default();
        registry.register(
            "subscribed_topic_peer_count",
            "Number of peers subscribed to each subscribed topic",
            Box::new(subscribed_topic_peers.clone()),
        );

        /* Router Metrics */

        // Invalid Topic Messages
        let invalid_topic_messages = Counter::default();
        registry.register(
            "invalid_topic_messages",
            "Number of times a message has been received on a non-subscribed topic",
            Box::new(invalid_topic_messages.clone()),
        );

        let memcache_misses = Counter::default();
        registry.register(
                "memcache_misses",
                "Number of times a message has attempted to be forwarded but has already been removed from the memcache",
                Box::new(memcache_misses.clone()),
                );

        InternalMetrics {
            mesh_peers,
            mesh_score,
            mesh_avg_score,
            mesh_message_rx_total,
            mesh_message_tx_total,
            mesh_messages_from_non_mesh_peers,
            mesh_duplicates_filtered,
            mesh_messages_validated,
            mesh_messages_rejected,
            mesh_messages_ignored,
            mesh_first_message_deliveries_per_slot,
            mesh_iwant_requests,
            broken_promises,
            memcache_misses,
            topic_peers,
            subscribed_topic_peers,
            invalid_topic_messages,
            topic_metrics: HashMap::new(),
            added_mesh_topics: HashSet::new(),
            added_non_mesh_topics: HashSet::new(),
        }
    }

    /// Access to the fine-grained metrics which provide information per-peer (slot) of the
    /// specified mesh topic.
    pub fn slot_metrics_for_topic(
        &self,
        topic: &TopicHash,
    ) -> Option<impl Iterator<Item = &SlotMetricCounts>> {
        Some(self.topic_metrics.get(topic)?.slot_metrics_iter())
    }

    /// Reports that an attempted message to forward was no longer in the memcache.
    pub fn memcache_miss(&mut self) {
        self.memcache_misses.inc();
    }

    /// Reports a broken promise.
    pub fn broken_promise(&mut self) {
        self.broken_promises.inc();
    }

    /// Reports that a message was published on the specified topic.
    pub fn message_published(&mut self, topic_hash: &TopicHash) {
        if self.allowed_mesh_topic(topic_hash) {
            self.mesh_message_tx_total.get_or_create(topic_hash).inc();
        }
    }

    /// Churns a slot in the topic_metrics. This assumes the peer is in the mesh.
    pub fn churn_slot(
        &mut self,
        topic_hash: &TopicHash,
        peer: &PeerId,
        slot_churn: SlotChurnMetric,
    ) {
        if let Ok(slot) = self
            .topic_metrics
            .entry(topic_hash.clone())
            .or_insert_with(|| TopicMetrics::new(topic_hash.clone()))
            .churn_slot(peer, slot_churn)
        {
            self.reset_slot(topic_hash, slot);
        }
    }

    pub fn iwant_request(&mut self, topic_hash: &TopicHash) {
        if self.allowed_mesh_topic(topic_hash) {
            self.mesh_iwant_requests.get_or_create(topic_hash).inc();
        }
    }

    pub fn message_invalid_topic(&mut self) {
        self.invalid_topic_messages.inc();
    }

    pub fn peer_joined_topic(&mut self, topic_hash: &TopicHash) {
        if self.allowed_non_mesh_topic(topic_hash) {
            self.topic_peers.get_or_create(topic_hash).inc();
        }
    }

    pub fn peer_joined_subscribed_topic(&mut self, topic_hash: &TopicHash) {
        if self.allowed_mesh_topic(topic_hash) {
            self.subscribed_topic_peers.get_or_create(topic_hash).inc();
        }
    }

    pub fn peer_left_topic(&mut self, topic_hash: &TopicHash) {
        if self.allowed_non_mesh_topic(topic_hash) {
            let v = self.topic_peers.get_or_create(topic_hash).get();
            self.topic_peers
                .get_or_create(topic_hash)
                .set(v.saturating_sub(1));
        }
    }

    pub fn peer_left_subscribed_topic(&mut self, topic_hash: &TopicHash) {
        // We use the non_mesh version here, because if we are subscribed this topic should exist
        // in the added_mesh_topics mappings
        if self.allowed_non_mesh_topic(topic_hash) {
            let v = self.subscribed_topic_peers.get_or_create(topic_hash).get();
            self.subscribed_topic_peers
                .get_or_create(topic_hash)
                .set(v.saturating_sub(1));
        }
    }

    pub fn leave_topic(&mut self, topic_hash: &TopicHash) {
        // Remove all the peers from all slots
        if let Some(metrics) = self.topic_metrics.get_mut(topic_hash) {
            let total_slots = metrics.churn_all_slots(SlotChurnMetric::ChurnLeave);
            // Remove the slot metrics
            if self.allowed_mesh_topic(topic_hash) {
                for slot in 1..total_slots {
                    self.reset_slot(topic_hash, Slot { slot });
                }
            }
        }

        if self.allowed_non_mesh_topic(topic_hash) {
            self.mesh_peers.get_or_create(topic_hash).set(0);
            self.mesh_avg_score.get_or_create(topic_hash).set(0);
            self.subscribed_topic_peers.get_or_create(topic_hash).set(0);
        }
    }

    fn reset_slot(&mut self, topic_hash: &TopicHash, slot: Slot) {
        if self.allowed_mesh_topic(topic_hash) {
            self.mesh_first_message_deliveries_per_slot
                .get_or_create(&(topic_hash.clone(), slot))
                .set(0);
        }
    }

    /// Helpful for testing and validation
    #[cfg(debug_assertions)]
    pub fn topic_metrics(&self) -> &HashMap<TopicHash, TopicMetrics> {
        &self.topic_metrics
    }

    /// Increment a MessageMetric in the topic_metrics for peer in topic.
    pub fn increment_message_metric(
        &mut self,
        topic_hash: &TopicHash,
        peer: &PeerId,
        message_metric: SlotMessageMetric,
    ) {
        if let Ok(slot) = self
            .topic_metrics
            .entry(topic_hash.clone())
            .or_insert_with(|| TopicMetrics::new(topic_hash.clone()))
            .increment_message_metric(peer, &message_metric)
        {
            if self.allowed_mesh_topic(topic_hash) {
                match message_metric {
                    SlotMessageMetric::MessagesAll => {}
                    SlotMessageMetric::MessagesDuplicates => {
                        self.mesh_duplicates_filtered
                            .get_or_create(topic_hash)
                            .inc();
                    }
                    SlotMessageMetric::MessagesFirst => {
                        self.mesh_message_rx_total.get_or_create(topic_hash).inc();
                        if slot.slot == 0 {
                            self.mesh_messages_from_non_mesh_peers
                                .get_or_create(topic_hash)
                                .inc();
                        } else {
                            self.mesh_first_message_deliveries_per_slot
                                .get_or_create(&(topic_hash.clone(), slot))
                                .inc();
                        }
                    }
                    SlotMessageMetric::MessagesIgnored => {
                        self.mesh_messages_ignored.get_or_create(topic_hash).inc();
                    }
                    SlotMessageMetric::MessagesRejected => {
                        self.mesh_messages_rejected.get_or_create(topic_hash).inc();
                    }
                    SlotMessageMetric::MessagesValidated => {
                        self.mesh_messages_validated.get_or_create(topic_hash).inc();
                    }
                }
            }
        }
    }

    /// Assign slots in topic to peers.
    pub fn assign_slots_to_peers<U>(&mut self, topic_hash: &TopicHash, peer_list: U)
    where
        U: Iterator<Item = PeerId>,
    {
        self.topic_metrics
            .entry(topic_hash.clone())
            .or_insert_with(|| TopicMetrics::new(topic_hash.clone()))
            .assign_slots_to_peers(peer_list);
    }

    /// Assigns a slot in topic to the peer if the peer doesn't already have one.
    pub fn assign_slot_if_unassigned(&mut self, topic: &TopicHash, peer: &PeerId) {
        self.topic_metrics
            .entry(topic.clone())
            .or_insert_with(|| TopicMetrics::new(topic.clone()))
            .assign_slot_if_unassigned(*peer);
    }

    /// Limits the number of topics that can be created in each metric. If the topic hasn't already
    /// been added to a metric and we have added less than TOPIC_LIMIT topics, this will return
    /// true.
    fn allowed_mesh_topic(&mut self, topic_hash: &TopicHash) -> bool {
        // If we haven't reached the limit, record the topic.
        if self.added_mesh_topics.len() < MESH_TOPIC_LIMIT {
            self.added_mesh_topics.insert(topic_hash.clone());
            true
        } else {
            // We've reached the topic limit, the topic is allowed if we have seen it before,
            // otherwise we reject it.
            self.added_mesh_topics.contains(topic_hash)
                | self.added_non_mesh_topics.contains(topic_hash)
        }
    }

    /// Limits the number of topics that can be created in each metric. If the topic hasn't already
    /// been added to a metric and we have added less than TOPIC_LIMIT topics, this will return
    /// true.
    fn allowed_non_mesh_topic(&mut self, topic_hash: &TopicHash) -> bool {
        // If we haven't reached the limit, record the topic.
        if self.added_non_mesh_topics.len() < NON_MESH_TOPIC_LIMIT
            && !self.added_mesh_topics.contains(topic_hash)
        {
            self.added_non_mesh_topics.insert(topic_hash.clone());
            true
        } else {
            // We've reached the topic limit, the topic is allowed if we have seen it before,
            // otherwise we reject it.
            self.added_non_mesh_topics.contains(topic_hash)
        }
    }
}
