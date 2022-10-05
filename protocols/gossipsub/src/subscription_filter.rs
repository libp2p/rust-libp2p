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

use crate::types::GossipsubSubscription;
use crate::TopicHash;
use log::debug;
use std::collections::{BTreeSet, HashMap, HashSet};

pub trait TopicSubscriptionFilter {
    /// Returns true iff the topic is of interest and we can subscribe to it.
    fn can_subscribe(&mut self, topic_hash: &TopicHash) -> bool;

    /// Filters a list of incoming subscriptions and returns a filtered set
    /// By default this deduplicates the subscriptions and calls
    /// [`Self::filter_incoming_subscription_set`] on the filtered set.
    fn filter_incoming_subscriptions<'a>(
        &mut self,
        subscriptions: &'a [GossipsubSubscription],
        currently_subscribed_topics: &BTreeSet<TopicHash>,
    ) -> Result<HashSet<&'a GossipsubSubscription>, String> {
        let mut filtered_subscriptions: HashMap<TopicHash, &GossipsubSubscription> = HashMap::new();
        for subscription in subscriptions {
            use std::collections::hash_map::Entry::*;
            match filtered_subscriptions.entry(subscription.topic_hash.clone()) {
                Occupied(entry) => {
                    if entry.get().action != subscription.action {
                        entry.remove();
                    }
                }
                Vacant(entry) => {
                    entry.insert(subscription);
                }
            }
        }
        self.filter_incoming_subscription_set(
            filtered_subscriptions.into_iter().map(|(_, v)| v).collect(),
            currently_subscribed_topics,
        )
    }

    /// Filters a set of deduplicated subscriptions
    /// By default this filters the elements based on [`Self::allow_incoming_subscription`].
    fn filter_incoming_subscription_set<'a>(
        &mut self,
        mut subscriptions: HashSet<&'a GossipsubSubscription>,
        _currently_subscribed_topics: &BTreeSet<TopicHash>,
    ) -> Result<HashSet<&'a GossipsubSubscription>, String> {
        subscriptions.retain(|s| {
            if self.allow_incoming_subscription(s) {
                true
            } else {
                debug!("Filtered incoming subscription {:?}", s);
                false
            }
        });
        Ok(subscriptions)
    }

    /// Returns true iff we allow an incoming subscription.
    /// This is used by the default implementation of filter_incoming_subscription_set to decide
    /// whether to filter out a subscription or not.
    /// By default this uses can_subscribe to decide the same for incoming subscriptions as for
    /// outgoing ones.
    fn allow_incoming_subscription(&mut self, subscription: &GossipsubSubscription) -> bool {
        self.can_subscribe(&subscription.topic_hash)
    }
}

//some useful implementers

/// Allows all subscriptions
#[derive(Default, Clone)]
pub struct AllowAllSubscriptionFilter {}

impl TopicSubscriptionFilter for AllowAllSubscriptionFilter {
    fn can_subscribe(&mut self, _: &TopicHash) -> bool {
        true
    }
}

/// Allows only whitelisted subscriptions
#[derive(Default, Clone)]
pub struct WhitelistSubscriptionFilter(pub HashSet<TopicHash>);

impl TopicSubscriptionFilter for WhitelistSubscriptionFilter {
    fn can_subscribe(&mut self, topic_hash: &TopicHash) -> bool {
        self.0.contains(topic_hash)
    }
}

/// Adds a max count to a given subscription filter
pub struct MaxCountSubscriptionFilter<T: TopicSubscriptionFilter> {
    pub filter: T,
    pub max_subscribed_topics: usize,
    pub max_subscriptions_per_request: usize,
}

impl<T: TopicSubscriptionFilter> TopicSubscriptionFilter for MaxCountSubscriptionFilter<T> {
    fn can_subscribe(&mut self, topic_hash: &TopicHash) -> bool {
        self.filter.can_subscribe(topic_hash)
    }

    fn filter_incoming_subscriptions<'a>(
        &mut self,
        subscriptions: &'a [GossipsubSubscription],
        currently_subscribed_topics: &BTreeSet<TopicHash>,
    ) -> Result<HashSet<&'a GossipsubSubscription>, String> {
        if subscriptions.len() > self.max_subscriptions_per_request {
            return Err("too many subscriptions per request".into());
        }
        let result = self
            .filter
            .filter_incoming_subscriptions(subscriptions, currently_subscribed_topics)?;

        use crate::types::GossipsubSubscriptionAction::*;

        let mut unsubscribed = 0;
        let mut new_subscribed = 0;
        for s in &result {
            let currently_contained = currently_subscribed_topics.contains(&s.topic_hash);
            match s.action {
                Unsubscribe => {
                    if currently_contained {
                        unsubscribed += 1;
                    }
                }
                Subscribe => {
                    if !currently_contained {
                        new_subscribed += 1;
                    }
                }
            }
        }

        if new_subscribed + currently_subscribed_topics.len()
            > self.max_subscribed_topics + unsubscribed
        {
            return Err("too many subscribed topics".into());
        }

        Ok(result)
    }
}

/// Combines two subscription filters
pub struct CombinedSubscriptionFilters<T: TopicSubscriptionFilter, S: TopicSubscriptionFilter> {
    pub filter1: T,
    pub filter2: S,
}

impl<T, S> TopicSubscriptionFilter for CombinedSubscriptionFilters<T, S>
where
    T: TopicSubscriptionFilter,
    S: TopicSubscriptionFilter,
{
    fn can_subscribe(&mut self, topic_hash: &TopicHash) -> bool {
        self.filter1.can_subscribe(topic_hash) && self.filter2.can_subscribe(topic_hash)
    }

    fn filter_incoming_subscription_set<'a>(
        &mut self,
        subscriptions: HashSet<&'a GossipsubSubscription>,
        currently_subscribed_topics: &BTreeSet<TopicHash>,
    ) -> Result<HashSet<&'a GossipsubSubscription>, String> {
        let intermediate = self
            .filter1
            .filter_incoming_subscription_set(subscriptions, currently_subscribed_topics)?;
        self.filter2
            .filter_incoming_subscription_set(intermediate, currently_subscribed_topics)
    }
}

pub struct CallbackSubscriptionFilter<T>(pub T)
where
    T: FnMut(&TopicHash) -> bool;

impl<T> TopicSubscriptionFilter for CallbackSubscriptionFilter<T>
where
    T: FnMut(&TopicHash) -> bool,
{
    fn can_subscribe(&mut self, topic_hash: &TopicHash) -> bool {
        (self.0)(topic_hash)
    }
}

pub mod regex {
    use super::TopicSubscriptionFilter;
    use crate::TopicHash;
    use regex::Regex;

    ///A subscription filter that filters topics based on a regular expression.
    pub struct RegexSubscriptionFilter(pub Regex);

    impl TopicSubscriptionFilter for RegexSubscriptionFilter {
        fn can_subscribe(&mut self, topic_hash: &TopicHash) -> bool {
            self.0.is_match(topic_hash.as_str())
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;
        use crate::types::GossipsubSubscription;
        use crate::types::GossipsubSubscriptionAction::*;

        #[test]
        fn test_regex_subscription_filter() {
            let t1 = TopicHash::from_raw("tt");
            let t2 = TopicHash::from_raw("et3t3te");
            let t3 = TopicHash::from_raw("abcdefghijklmnopqrsuvwxyz");

            let mut filter = RegexSubscriptionFilter(Regex::new("t.*t").unwrap());

            let old = Default::default();
            let subscriptions = vec![
                GossipsubSubscription {
                    action: Subscribe,
                    topic_hash: t1,
                },
                GossipsubSubscription {
                    action: Subscribe,
                    topic_hash: t2,
                },
                GossipsubSubscription {
                    action: Subscribe,
                    topic_hash: t3,
                },
            ];

            let result = filter
                .filter_incoming_subscriptions(&subscriptions, &old)
                .unwrap();
            assert_eq!(result, subscriptions[..2].iter().collect());
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::types::GossipsubSubscriptionAction::*;
    use std::iter::FromIterator;

    #[test]
    fn test_filter_incoming_allow_all_with_duplicates() {
        let mut filter = AllowAllSubscriptionFilter {};

        let t1 = TopicHash::from_raw("t1");
        let t2 = TopicHash::from_raw("t2");

        let old = BTreeSet::from_iter(vec![t1.clone()].into_iter());
        let subscriptions = vec![
            GossipsubSubscription {
                action: Unsubscribe,
                topic_hash: t1.clone(),
            },
            GossipsubSubscription {
                action: Unsubscribe,
                topic_hash: t2.clone(),
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t2,
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t1.clone(),
            },
            GossipsubSubscription {
                action: Unsubscribe,
                topic_hash: t1,
            },
        ];

        let result = filter
            .filter_incoming_subscriptions(&subscriptions, &old)
            .unwrap();
        assert_eq!(result, vec![&subscriptions[4]].into_iter().collect());
    }

    #[test]
    fn test_filter_incoming_whitelist() {
        let t1 = TopicHash::from_raw("t1");
        let t2 = TopicHash::from_raw("t2");

        let mut filter = WhitelistSubscriptionFilter(HashSet::from_iter(vec![t1.clone()]));

        let old = Default::default();
        let subscriptions = vec![
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t1,
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t2,
            },
        ];

        let result = filter
            .filter_incoming_subscriptions(&subscriptions, &old)
            .unwrap();
        assert_eq!(result, vec![&subscriptions[0]].into_iter().collect());
    }

    #[test]
    fn test_filter_incoming_too_many_subscriptions_per_request() {
        let t1 = TopicHash::from_raw("t1");

        let mut filter = MaxCountSubscriptionFilter {
            filter: AllowAllSubscriptionFilter {},
            max_subscribed_topics: 100,
            max_subscriptions_per_request: 2,
        };

        let old = Default::default();

        let subscriptions = vec![
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t1.clone(),
            },
            GossipsubSubscription {
                action: Unsubscribe,
                topic_hash: t1.clone(),
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t1,
            },
        ];

        let result = filter.filter_incoming_subscriptions(&subscriptions, &old);
        assert_eq!(result, Err("too many subscriptions per request".into()));
    }

    #[test]
    fn test_filter_incoming_too_many_subscriptions() {
        let t: Vec<_> = (0..4)
            .map(|i| TopicHash::from_raw(format!("t{}", i)))
            .collect();

        let mut filter = MaxCountSubscriptionFilter {
            filter: AllowAllSubscriptionFilter {},
            max_subscribed_topics: 3,
            max_subscriptions_per_request: 2,
        };

        let old = t[0..2].iter().cloned().collect();

        let subscriptions = vec![
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t[2].clone(),
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t[3].clone(),
            },
        ];

        let result = filter.filter_incoming_subscriptions(&subscriptions, &old);
        assert_eq!(result, Err("too many subscribed topics".into()));
    }

    #[test]
    fn test_filter_incoming_max_subscribed_valid() {
        let t: Vec<_> = (0..5)
            .map(|i| TopicHash::from_raw(format!("t{}", i)))
            .collect();

        let mut filter = MaxCountSubscriptionFilter {
            filter: WhitelistSubscriptionFilter(t.iter().take(4).cloned().collect()),
            max_subscribed_topics: 2,
            max_subscriptions_per_request: 5,
        };

        let old = t[0..2].iter().cloned().collect();

        let subscriptions = vec![
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t[4].clone(),
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t[2].clone(),
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t[3].clone(),
            },
            GossipsubSubscription {
                action: Unsubscribe,
                topic_hash: t[0].clone(),
            },
            GossipsubSubscription {
                action: Unsubscribe,
                topic_hash: t[1].clone(),
            },
        ];

        let result = filter
            .filter_incoming_subscriptions(&subscriptions, &old)
            .unwrap();
        assert_eq!(result, subscriptions[1..].iter().collect());
    }

    #[test]
    fn test_callback_filter() {
        let t1 = TopicHash::from_raw("t1");
        let t2 = TopicHash::from_raw("t2");

        let mut filter = CallbackSubscriptionFilter(|h| h.as_str() == "t1");

        let old = Default::default();
        let subscriptions = vec![
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t1,
            },
            GossipsubSubscription {
                action: Subscribe,
                topic_hash: t2,
            },
        ];

        let result = filter
            .filter_incoming_subscriptions(&subscriptions, &old)
            .unwrap();
        assert_eq!(result, vec![&subscriptions[0]].into_iter().collect());
    }
}
