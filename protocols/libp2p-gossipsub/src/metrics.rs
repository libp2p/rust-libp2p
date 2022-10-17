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

use std::collections::HashMap;

use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::{Family, MetricConstructor};
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{linear_buckets, Histogram};
use prometheus_client::registry::Registry;

use crate::topic::TopicHash;
use crate::types::{MessageAcceptance, PeerKind};

// Default value that limits for how many topics do we store metrics.
const DEFAULT_MAX_TOPICS: usize = 300;

// Default value that limits how many topics for which there has never been a subscription do we
// store metrics.
const DEFAULT_MAX_NEVER_SUBSCRIBED_TOPICS: usize = 50;

#[derive(Debug, Clone)]
pub struct Config {
    /// This provides an upper bound to the number of mesh topics we create metrics for. It
    /// prevents unbounded labels being created in the metrics.
    pub max_topics: usize,
    /// Mesh topics are controlled by the user via subscriptions whereas non-mesh topics are
    /// determined by users on the network.  This limit permits a fixed amount of topics to allow,
    /// in-addition to the mesh topics.
    pub max_never_subscribed_topics: usize,
    /// Buckets used for the score histograms.
    pub score_buckets: Vec<f64>,
}

impl Config {
    /// Create buckets for the score histograms based on score thresholds.
    pub fn buckets_using_scoring_thresholds(&mut self, params: &crate::PeerScoreThresholds) {
        self.score_buckets = vec![
            params.graylist_threshold,
            params.publish_threshold,
            params.gossip_threshold,
            params.gossip_threshold / 2.0,
            params.gossip_threshold / 4.0,
            0.0,
            1.0,
            10.0,
            100.0,
        ];
    }
}

impl Default for Config {
    fn default() -> Self {
        // Some sensible defaults
        let gossip_threshold = -4000.0;
        let publish_threshold = -8000.0;
        let graylist_threshold = -16000.0;
        let score_buckets: Vec<f64> = vec![
            graylist_threshold,
            publish_threshold,
            gossip_threshold,
            gossip_threshold / 2.0,
            gossip_threshold / 4.0,
            0.0,
            1.0,
            10.0,
            100.0,
        ];
        Config {
            max_topics: DEFAULT_MAX_TOPICS,
            max_never_subscribed_topics: DEFAULT_MAX_NEVER_SUBSCRIBED_TOPICS,
            score_buckets,
        }
    }
}

/// Whether we have ever been subscribed to this topic.
type EverSubscribed = bool;

/// A collection of metrics used throughout the Gossipsub behaviour.
pub struct Metrics {
    /* Configuration parameters */
    /// Maximum number of topics for which we store metrics. This helps keep the metrics bounded.
    max_topics: usize,
    /// Maximum number of topics for which we store metrics, where the topic in not one to which we
    /// have subscribed at some point. This helps keep the metrics bounded, since these topics come
    /// from received messages and not explicit application subscriptions.
    max_never_subscribed_topics: usize,

    /* Auxiliary variables */
    /// Information needed to decide if a topic is allowed or not.
    topic_info: HashMap<TopicHash, EverSubscribed>,

    /* Metrics per known topic */
    /// Status of our subscription to this topic. This metric allows analyzing other topic metrics
    /// filtered by our current subscription status.
    topic_subscription_status: Family<TopicHash, Gauge>,
    /// Number of peers subscribed to each topic. This allows us to analyze a topic's behaviour
    /// regardless of our subscription status.
    topic_peers_count: Family<TopicHash, Gauge>,
    /// The number of invalid messages received for a given topic.
    invalid_messages: Family<TopicHash, Counter>,
    /// The number of messages accepted by the application (validation result).
    accepted_messages: Family<TopicHash, Counter>,
    /// The number of messages ignored by the application (validation result).
    ignored_messages: Family<TopicHash, Counter>,
    /// The number of messages rejected by the application (validation result).
    rejected_messages: Family<TopicHash, Counter>,

    /* Metrics regarding mesh state */
    /// Number of peers in our mesh. This metric should be updated with the count of peers for a
    /// topic in the mesh regardless of inclusion and churn events.
    mesh_peer_counts: Family<TopicHash, Gauge>,
    /// Number of times we include peers in a topic mesh for different reasons.
    mesh_peer_inclusion_events: Family<InclusionLabel, Counter>,
    /// Number of times we remove peers in a topic mesh for different reasons.
    mesh_peer_churn_events: Family<ChurnLabel, Counter>,

    /* Metrics regarding messages sent/received */
    /// Number of gossip messages sent to each topic.
    topic_msg_sent_counts: Family<TopicHash, Counter>,
    /// Bytes from gossip messages sent to each topic.
    topic_msg_sent_bytes: Family<TopicHash, Counter>,
    /// Number of gossipsub messages published to each topic.
    topic_msg_published: Family<TopicHash, Counter>,

    /// Number of gossipsub messages received on each topic (without filtering duplicates).
    topic_msg_recv_counts_unfiltered: Family<TopicHash, Counter>,
    /// Number of gossipsub messages received on each topic (after filtering duplicates).
    topic_msg_recv_counts: Family<TopicHash, Counter>,
    /// Bytes received from gossip messages for each topic.
    topic_msg_recv_bytes: Family<TopicHash, Counter>,

    /* Metrics related to scoring */
    /// Histogram of the scores for each mesh topic.
    score_per_mesh: Family<TopicHash, Histogram, HistBuilder>,
    /// A counter of the kind of penalties being applied to peers.
    scoring_penalties: Family<PenaltyLabel, Counter>,

    /* General Metrics */
    /// Gossipsub supports floodsub, gossipsub v1.0 and gossipsub v1.1. Peers are classified based
    /// on which protocol they support. This metric keeps track of the number of peers that are
    /// connected of each type.
    peers_per_protocol: Family<ProtocolLabel, Gauge>,
    /// The time it takes to complete one iteration of the heartbeat.
    heartbeat_duration: Histogram,

    /* Performance metrics */
    /// When the user validates a message, it tries to re propagate it to its mesh peers. If the
    /// message expires from the memcache before it can be validated, we count this a cache miss
    /// and it is an indicator that the memcache size should be increased.
    memcache_misses: Counter,
    /// The number of times we have decided that an IWANT control message is required for this
    /// topic. A very high metric might indicate an underperforming network.
    topic_iwant_msgs: Family<TopicHash, Counter>,
}

impl Metrics {
    pub fn new(registry: &mut Registry, config: Config) -> Self {
        // Destructure the config to be sure everything is used.
        let Config {
            max_topics,
            max_never_subscribed_topics,
            score_buckets,
        } = config;

        macro_rules! register_family {
            ($name:expr, $help:expr) => {{
                let fam = Family::default();
                registry.register($name, $help, Box::new(fam.clone()));
                fam
            }};
        }

        let topic_subscription_status = register_family!(
            "topic_subscription_status",
            "Subscription status per known topic"
        );
        let topic_peers_count = register_family!(
            "topic_peers_counts",
            "Number of peers subscribed to each topic"
        );

        let invalid_messages = register_family!(
            "invalid_messages_per_topic",
            "Number of invalid messages received for each topic"
        );

        let accepted_messages = register_family!(
            "accepted_messages_per_topic",
            "Number of accepted messages received for each topic"
        );

        let ignored_messages = register_family!(
            "ignored_messages_per_topic",
            "Number of ignored messages received for each topic"
        );

        let rejected_messages = register_family!(
            "rejected_messages_per_topic",
            "Number of rejected messages received for each topic"
        );

        let mesh_peer_counts = register_family!(
            "mesh_peer_counts",
            "Number of peers in each topic in our mesh"
        );
        let mesh_peer_inclusion_events = register_family!(
            "mesh_peer_inclusion_events",
            "Number of times a peer gets added to our mesh for different reasons"
        );
        let mesh_peer_churn_events = register_family!(
            "mesh_peer_churn_events",
            "Number of times a peer gets removed from our mesh for different reasons"
        );
        let topic_msg_sent_counts = register_family!(
            "topic_msg_sent_counts",
            "Number of gossip messages sent to each topic"
        );
        let topic_msg_published = register_family!(
            "topic_msg_published",
            "Number of gossip messages published to each topic"
        );
        let topic_msg_sent_bytes = register_family!(
            "topic_msg_sent_bytes",
            "Bytes from gossip messages sent to each topic"
        );

        let topic_msg_recv_counts_unfiltered = register_family!(
            "topic_msg_recv_counts_unfiltered",
            "Number of gossip messages received on each topic (without duplicates being filtered)"
        );

        let topic_msg_recv_counts = register_family!(
            "topic_msg_recv_counts",
            "Number of gossip messages received on each topic (after duplicates have been filtered)"
        );
        let topic_msg_recv_bytes = register_family!(
            "topic_msg_recv_bytes",
            "Bytes received from gossip messages for each topic"
        );

        let hist_builder = HistBuilder {
            buckets: score_buckets,
        };

        let score_per_mesh: Family<_, _, HistBuilder> = Family::new_with_constructor(hist_builder);
        registry.register(
            "score_per_mesh",
            "Histogram of scores per mesh topic",
            Box::new(score_per_mesh.clone()),
        );

        let scoring_penalties = register_family!(
            "scoring_penalties",
            "Counter of types of scoring penalties given to peers"
        );
        let peers_per_protocol = register_family!(
            "peers_per_protocol",
            "Number of connected peers by protocol type"
        );

        let heartbeat_duration = Histogram::new(linear_buckets(0.0, 50.0, 10));
        registry.register(
            "heartbeat_duration",
            "Histogram of observed heartbeat durations",
            Box::new(heartbeat_duration.clone()),
        );

        let topic_iwant_msgs = register_family!(
            "topic_iwant_msgs",
            "Number of times we have decided an IWANT is required for this topic"
        );
        let memcache_misses = {
            let metric = Counter::default();
            registry.register(
                "memcache_misses",
                "Number of times a message is not found in the duplicate cache when validating",
                Box::new(metric.clone()),
            );
            metric
        };

        Self {
            max_topics,
            max_never_subscribed_topics,
            topic_info: HashMap::default(),
            topic_subscription_status,
            topic_peers_count,
            invalid_messages,
            accepted_messages,
            ignored_messages,
            rejected_messages,
            mesh_peer_counts,
            mesh_peer_inclusion_events,
            mesh_peer_churn_events,
            topic_msg_sent_counts,
            topic_msg_sent_bytes,
            topic_msg_published,
            topic_msg_recv_counts_unfiltered,
            topic_msg_recv_counts,
            topic_msg_recv_bytes,
            score_per_mesh,
            scoring_penalties,
            peers_per_protocol,
            heartbeat_duration,
            memcache_misses,
            topic_iwant_msgs,
        }
    }

    fn non_subscription_topics_count(&self) -> usize {
        self.topic_info
            .values()
            .filter(|&ever_subscribed| !ever_subscribed)
            .count()
    }

    /// Registers a topic if not already known and if the bounds allow it.
    fn register_topic(&mut self, topic: &TopicHash) -> Result<(), ()> {
        if self.topic_info.contains_key(topic) {
            Ok(())
        } else if self.topic_info.len() < self.max_topics
            && self.non_subscription_topics_count() < self.max_never_subscribed_topics
        {
            // This is a topic without an explicit subscription and we register it if we are within
            // the configured bounds.
            self.topic_info.entry(topic.clone()).or_insert(false);
            self.topic_subscription_status.get_or_create(topic).set(0);
            Ok(())
        } else {
            // We don't know this topic and there is no space left to store it
            Err(())
        }
    }

    /// Register how many peers do we known are subscribed to this topic.
    pub fn set_topic_peers(&mut self, topic: &TopicHash, count: usize) {
        if self.register_topic(topic).is_ok() {
            self.topic_peers_count
                .get_or_create(topic)
                .set(count as u64);
        }
    }

    /* Mesh related methods */

    /// Registers the subscription to a topic if the configured limits allow it.
    /// Sets the registered number of peers in the mesh to 0.
    pub fn joined(&mut self, topic: &TopicHash) {
        if self.topic_info.contains_key(topic) || self.topic_info.len() < self.max_topics {
            self.topic_info.insert(topic.clone(), true);
            let was_subscribed = self.topic_subscription_status.get_or_create(topic).set(1);
            debug_assert_eq!(was_subscribed, 0);
            self.mesh_peer_counts.get_or_create(topic).set(0);
        }
    }

    /// Registers the unsubscription to a topic if the topic was previously allowed.
    /// Sets the registered number of peers in the mesh to 0.
    pub fn left(&mut self, topic: &TopicHash) {
        if self.topic_info.contains_key(topic) {
            // Depending on the configured topic bounds we could miss a mesh topic.
            // So, check first if the topic was previously allowed.
            let was_subscribed = self.topic_subscription_status.get_or_create(topic).set(0);
            debug_assert_eq!(was_subscribed, 1);
            self.mesh_peer_counts.get_or_create(topic).set(0);
        }
    }

    /// Register the inclusion of peers in our mesh due to some reason.
    pub fn peers_included(&mut self, topic: &TopicHash, reason: Inclusion, count: usize) {
        if self.register_topic(topic).is_ok() {
            self.mesh_peer_inclusion_events
                .get_or_create(&InclusionLabel {
                    hash: topic.to_string(),
                    reason,
                })
                .inc_by(count as u64);
        }
    }

    /// Register the removal of peers in our mesh due to some reason.
    pub fn peers_removed(&mut self, topic: &TopicHash, reason: Churn, count: usize) {
        if self.register_topic(topic).is_ok() {
            self.mesh_peer_churn_events
                .get_or_create(&ChurnLabel {
                    hash: topic.to_string(),
                    reason,
                })
                .inc_by(count as u64);
        }
    }

    /// Register the current number of peers in our mesh for this topic.
    pub fn set_mesh_peers(&mut self, topic: &TopicHash, count: usize) {
        if self.register_topic(topic).is_ok() {
            // Due to limits, this topic could have not been allowed, so we check.
            self.mesh_peer_counts.get_or_create(topic).set(count as u64);
        }
    }

    /// Register that an invalid message was received on a specific topic.
    pub fn register_invalid_message(&mut self, topic: &TopicHash) {
        if self.register_topic(topic).is_ok() {
            self.invalid_messages.get_or_create(topic).inc();
        }
    }

    /// Register a score penalty.
    pub fn register_score_penalty(&mut self, penalty: Penalty) {
        self.scoring_penalties
            .get_or_create(&PenaltyLabel { penalty })
            .inc();
    }

    /// Registers that a message was published on a specific topic.
    pub fn register_published_message(&mut self, topic: &TopicHash) {
        if self.register_topic(topic).is_ok() {
            self.topic_msg_published.get_or_create(topic).inc();
        }
    }

    /// Register sending a message over a topic.
    pub fn msg_sent(&mut self, topic: &TopicHash, bytes: usize) {
        if self.register_topic(topic).is_ok() {
            self.topic_msg_sent_counts.get_or_create(topic).inc();
            self.topic_msg_sent_bytes
                .get_or_create(topic)
                .inc_by(bytes as u64);
        }
    }

    /// Register that a message was received (and was not a duplicate).
    pub fn msg_recvd(&mut self, topic: &TopicHash) {
        if self.register_topic(topic).is_ok() {
            self.topic_msg_recv_counts.get_or_create(topic).inc();
        }
    }

    /// Register that a message was received (could have been a duplicate).
    pub fn msg_recvd_unfiltered(&mut self, topic: &TopicHash, bytes: usize) {
        if self.register_topic(topic).is_ok() {
            self.topic_msg_recv_counts_unfiltered
                .get_or_create(topic)
                .inc();
            self.topic_msg_recv_bytes
                .get_or_create(topic)
                .inc_by(bytes as u64);
        }
    }

    pub fn register_msg_validation(&mut self, topic: &TopicHash, validation: &MessageAcceptance) {
        if self.register_topic(topic).is_ok() {
            match validation {
                MessageAcceptance::Accept => self.accepted_messages.get_or_create(topic).inc(),
                MessageAcceptance::Ignore => self.ignored_messages.get_or_create(topic).inc(),
                MessageAcceptance::Reject => self.rejected_messages.get_or_create(topic).inc(),
            };
        }
    }

    /// Register a memcache miss.
    pub fn memcache_miss(&mut self) {
        self.memcache_misses.inc();
    }

    /// Register sending an IWANT msg for this topic.
    pub fn register_iwant(&mut self, topic: &TopicHash) {
        if self.register_topic(topic).is_ok() {
            self.topic_iwant_msgs.get_or_create(topic).inc();
        }
    }

    /// Observes a heartbeat duration.
    pub fn observe_heartbeat_duration(&mut self, millis: u64) {
        self.heartbeat_duration.observe(millis as f64);
    }

    /// Observe a score of a mesh peer.
    pub fn observe_mesh_peers_score(&mut self, topic: &TopicHash, score: f64) {
        if self.register_topic(topic).is_ok() {
            self.score_per_mesh.get_or_create(topic).observe(score);
        }
    }

    /// Register a new peers connection based on its protocol.
    pub fn peer_protocol_connected(&mut self, kind: PeerKind) {
        self.peers_per_protocol
            .get_or_create(&ProtocolLabel { protocol: kind })
            .inc();
    }

    /// Removes a peer from the counter based on its protocol when it disconnects.
    pub fn peer_protocol_disconnected(&mut self, kind: PeerKind) {
        let metric = self
            .peers_per_protocol
            .get_or_create(&ProtocolLabel { protocol: kind });
        if metric.get() != 0 {
            // decrement the counter
            metric.set(metric.get() - 1);
        }
    }
}

/// Reasons why a peer was included in the mesh.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
pub enum Inclusion {
    /// Peer was a fanaout peer.
    Fanout,
    /// Included from random selection.
    Random,
    /// Peer subscribed.
    Subscribed,
    /// Peer was included to fill the outbound quota.
    Outbound,
}

/// Reasons why a peer was removed from the mesh.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
pub enum Churn {
    /// Peer disconnected.
    Dc,
    /// Peer had a bad score.
    BadScore,
    /// Peer sent a PRUNE.
    Prune,
    /// Peer unsubscribed.
    Unsub,
    /// Too many peers.
    Excess,
}

/// Kinds of reasons a peer's score has been penalized
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
pub enum Penalty {
    /// A peer grafted before waiting the back-off time.
    GraftBackoff,
    /// A Peer did not respond to an IWANT request in time.
    BrokenPromise,
    /// A Peer did not send enough messages as expected.
    MessageDeficit,
    /// Too many peers under one IP address.
    IPColocation,
}

/// Label for the mesh inclusion event metrics.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
struct InclusionLabel {
    hash: String,
    reason: Inclusion,
}

/// Label for the mesh churn event metrics.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
struct ChurnLabel {
    hash: String,
    reason: Churn,
}

/// Label for the kinds of protocols peers can connect as.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
struct ProtocolLabel {
    protocol: PeerKind,
}

/// Label for the kinds of scoring penalties that can occur
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
struct PenaltyLabel {
    penalty: Penalty,
}

#[derive(Clone)]
struct HistBuilder {
    buckets: Vec<f64>,
}

impl MetricConstructor<Histogram> for HistBuilder {
    fn new_metric(&self) -> Histogram {
        Histogram::new(self.buckets.clone().into_iter())
    }
}
