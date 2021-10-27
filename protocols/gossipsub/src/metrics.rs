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

// pub mod topic_metrics;

use crate::topic::TopicHash;
use std::collections::HashMap;

use open_metrics_client::encoding::text::Encode;
use open_metrics_client::metrics::counter::Counter;
use open_metrics_client::metrics::family::{Family, MetricConstructor};
use open_metrics_client::metrics::gauge::Gauge;
use open_metrics_client::metrics::histogram::{linear_buckets, Histogram};
use open_metrics_client::registry::Registry;

// Default value that limits how many topics for which there has been a previous or current
// subscription do we store metrics.
const DEFAULT_MAX_EXPLICITLY_SUBSCRIBED_TOPICS: usize = 300;

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
            max_topics: DEFAULT_MAX_EXPLICITLY_SUBSCRIBED_TOPICS,
            max_never_subscribed_topics: DEFAULT_MAX_NEVER_SUBSCRIBED_TOPICS,
        }
    }
}

// Whether we have ever been subscribed to this topic.
type EverSubscribed = bool;

/// A collection of metrics used throughout the Gossipsub behaviour.
pub struct InternalMetrics {
    /*
     * Configuration parameters
     */
    /// Maximum number of topics for which we store metrics. This helps keep the metrics bounded.
    max_topics: usize,
    /// Maximum number of topics for which we store metrics, where the topic in not one to which we
    /// have subscribed at some point. This helps keep the metrics bounded, since these topics come
    /// from received messages and not explicit application subscriptions.
    max_never_subscribed_topics: usize,
    /*
     * Auxiliary variables
     */
    /// Information needed to decide if a topic is allowed or not.
    topic_info: HashMap<TopicHash, EverSubscribed>,
    /*
     * Metrics per topic
     */
    /// Status of our subscription to this topic. This metric allows anallyzing other topic metrics
    /// filtered by our current subscription status.
    topic_subscription_status: Family<TopicHash, Gauge>,
    /// Number of peers subscribed to each topic.
    topic_peers_count: Family<TopicHash, Gauge>,
    /// Number of iwant requests that we have sent for each topic. A large number indicates the
    /// mesh isn't performing as optimally as we would like and we have had to request extra
    /// messages via gossip.
    iwant_sent_requests: Family<TopicHash, Counter>,
}

impl InternalMetrics {
    pub fn new(registry: &mut Registry, config: Config) -> Self {
        // Destructure the config to be sure everything is used.
        let Config {
            max_topics,
            max_never_subscribed_topics,
        } = config;

        let topic_subscription_status = Family::default();
        registry.register(
            "topic_subscription_status",
            "Subscription status per topic",
            Box::new(topic_subscription_status.clone()),
        );

        let topic_peers_count = Family::default();
        registry.register(
            "topic_peers_counts",
            "Number of peers subscribed to each topic",
            Box::new(topic_peers_count.clone()),
        );

        let iwant_sent_requests = Family::default();
        registry.register(
            "iwant_sent_requests",
            "The number of IWANT requests we have sent per mesh",
            Box::new(iwant_sent_requests.clone()),
        );

        Self {
            max_topics,
            max_never_subscribed_topics,
            topic_info: HashMap::default(),
            topic_subscription_status,
            topic_peers_count,
            iwant_sent_requests,
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
        // Get the topic if we have seem it before. If not, this is as topic without an explicit
        // subscription and we register it if we are whithin the configured bounds.
        if self.topic_info.contains_key(topic)
            || self.non_subscription_topics_count() < self.max_never_subscribed_topics
        {
            self.topic_info.entry(topic.clone()).or_insert(false);
            self.topic_subscription_status.get_or_create(topic).set(0);
            Ok(())
        } else {
            Err(())
        }
    }

    /// Registers a peer's subscription to a topic.
    pub fn peer_joined_topic(&mut self, topic: &TopicHash) {
        if let Ok(info) = self.register_topic(topic) {}
    }

    /// Register an iwant sent request for a topic.
    pub fn iwant_sent(&mut self, topic: &TopicHash) {
        if self.register_topic(topic).is_ok() {
            self.iwant_sent_requests.get_or_create(topic).inc();
        }
    }

    /// Registers the subscription to a topic if the configured limits allow it.
    pub fn subscribed(&mut self, topic: &TopicHash) {
        let allowed = if let Some(ever_subscribed) = self.topic_info.get_mut(topic) {
            *ever_subscribed = true;
            true
        } else
        // First time we see, check if we allow it
        if self.topic_info.len() < self.max_topics {
            self.topic_info.insert(topic.clone(), true);
            true
        } else {
            false
        };

        if allowed {
            let was_subscribed = self.topic_subscription_status.get_or_create(topic).set(1);
            debug_assert_eq!(was_subscribed, 0);
        }
    }

    /// Registers the unsubscription to a topic if the topic was previously allowed.
    pub fn unsubscribed(&mut self, topic: &TopicHash) {
        if self.topic_info.contains_key(topic) {
            // Depending on the configured topic bounds we could miss a mesh topic's subscription.
            // So, check first if the topic was previously allowed.
            let was_subscribed = self.topic_subscription_status.get_or_create(topic).set(0);
            debug_assert_eq!(was_subscribed, 1);
        }
    }

}
