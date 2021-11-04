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

use open_metrics_client::encoding::text::Encode;
use open_metrics_client::metrics::counter::Counter;
use open_metrics_client::metrics::family::Family;
use open_metrics_client::metrics::gauge::Gauge;
use open_metrics_client::registry::Registry;

use crate::topic::TopicHash;

// Default value that limits for how many topics do we store metrics.
const DEFAULT_MAX_TOPICS: usize = 300;

// Default value that limits how many topics for which there has never been a subscription do we
// store metrics.
const DEFAULT_MAX_NEVER_SUBSCRIBED_TOPICS: usize = 50;

pub struct Config {
    /// This provides an upper bound to the number of mesh topics we create metrics for. It
    /// prevents unbounded labels being created in the metrics.
    pub max_topics: usize,
    /// Mesh topics are controlled by the user via subscriptions whereas non-mesh topics are
    /// determined by users on the network.  This limit permits a fixed amount of topics to allow,
    /// in-addition to the mesh topics.
    pub max_never_subscribed_topics: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_topics: DEFAULT_MAX_TOPICS,
            max_never_subscribed_topics: DEFAULT_MAX_NEVER_SUBSCRIBED_TOPICS,
        }
    }
}

/// Whether we have ever been subscribed to this topic.
type EverSubscribed = bool;

/// Reasons why a peer was included in the mesh.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
pub enum Inclusion {
    /// Peer was a fanaout peer.
    Fanaout,
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

/// Label for the mesh inclusion event metrics.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
struct InclusionLabel {
    topic: TopicHash,
    reason: Inclusion,
}

/// Label for the mesh churn event metrics.
#[derive(PartialEq, Eq, Hash, Encode, Clone)]
struct ChurnLabel {
    topic: TopicHash,
    reason: Churn,
}

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

    /* Metrics regarding mesh state */
    /// Number of peers in our mesh. This metric should be updated with the count of peers for a
    /// topic in the mesh regardless of inclusion and churn events.
    mesh_peer_counts: Family<TopicHash, Gauge>,
    /// Number of times we include peers in a topic mesh for different reasons.
    mesh_peer_inclusion_events: Family<InclusionLabel, Counter>,
    /// Number of times we remove peers in a topic mesh for different reasons.
    mesh_peer_churn_events: Family<ChurnLabel, Counter>,

    /* Metrics regarding messages sent */
    /// Number of gossip messages sent to each topic.
    topic_msg_sent_counts: Family<TopicHash, Counter>,
    /// Bytes from gossip messages sent to each topic .
    topic_msg_sent_bytes: Family<TopicHash, Counter>,
}

impl Metrics {
    pub fn new(registry: &mut Registry, config: Config) -> Self {
        // Destructure the config to be sure everything is used.
        let Config {
            max_topics,
            max_never_subscribed_topics,
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
            "Number of gossip messages sent to each topic."
        );
        let topic_msg_sent_bytes = register_family!(
            "topic_msg_sent_bytes",
            "Bytes from gossip messages sent to each topic."
        );

        Self {
            max_topics,
            max_never_subscribed_topics,
            topic_info: HashMap::default(),
            topic_subscription_status,
            topic_peers_count,
            mesh_peer_counts,
            mesh_peer_inclusion_events,
            mesh_peer_churn_events,
            topic_msg_sent_counts,
            topic_msg_sent_bytes,
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
                    topic: topic.clone(),
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
                    topic: topic.clone(),
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

    /// Register sending a message over a topic.
    pub fn msg_sent(&mut self, topic: &TopicHash, bytes: usize) {
        if self.register_topic(topic).is_ok() {
            self.topic_msg_sent_counts.get_or_create(topic).inc();
            self.topic_msg_sent_bytes
                .get_or_create(topic)
                .inc_by(bytes as u64);
        }
    }
}
