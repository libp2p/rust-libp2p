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

extern crate fnv;

use crate::protocol::GossipsubMessage;
use crate::topic::TopicHash;
use std::collections::HashMap;

/// CacheEntry stored in the history.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheEntry {
    mid: String,
    topics: Vec<TopicHash>,
}

/// MessageCache struct holding history of messages.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageCache {
    msgs: HashMap<String, GossipsubMessage>,
    history: Vec<Vec<CacheEntry>>,
    gossip: usize,
}

/// Implementation of the MessageCache.
impl MessageCache {
    pub fn new(gossip: usize, history_capacity: usize) -> MessageCache {
        MessageCache {
            gossip,
            msgs: HashMap::default(),
            history: vec![Vec::new(); history_capacity],
        }
    }

    /// Put a message into the memory cache
    pub fn put(&mut self, msg: GossipsubMessage) {
        let message_id = msg.id();
        let cache_entry = CacheEntry {
            mid: message_id.clone(),
            topics: msg.topics.clone(),
        };

        self.msgs.insert(message_id, msg);

        self.history[0].push(cache_entry);
    }

    /// Get a message with `message_id`
    pub fn get(&self, message_id: &str) -> Option<&GossipsubMessage> {
        self.msgs.get(message_id)
    }

    /// Get a list of GossipIds for a given topic
    pub fn get_gossip_ids(&self, topic: &TopicHash) -> Vec<String> {
        self.history[..self.gossip]
            .iter()
            .fold(vec![], |mut current_entries, entries| {
                // search for entries with desired topic
                let mut found_entries: Vec<String> = entries
                    .iter()
                    .filter_map(|entry| {
                        if entry.topics.iter().any(|t| t == topic) {
                            Some(entry.mid.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                // generate the list
                current_entries.append(&mut found_entries);
                current_entries
            })
    }

    /// Shift the history array down one and delete messages associated with the
    /// last entry
    pub fn shift(&mut self) {
        for entry in self.history.pop().expect("history is always > 1") {
            self.msgs.remove(&entry.mid);
        }

        // Insert an empty vec in position 0
        self.history.insert(0, Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Topic, TopicHash};
    use libp2p_core::PeerId;

    fn gen_testm(x: u64, topics: Vec<TopicHash>) -> GossipsubMessage {
        let u8x: u8 = x as u8;
        let source = PeerId::random();
        let data: Vec<u8> = vec![u8x];
        let sequence_number = x;

        let m = GossipsubMessage {
            source,
            data,
            sequence_number,
            topics,
        };
        m
    }

    #[test]
    /// Test that the message cache can be created.
    fn test_new_cache() {
        let x: usize = 3;
        let mc = MessageCache::new(x, 5);

        assert_eq!(mc.gossip, x);
    }

    #[test]
    /// Test you can put one message and get one.
    fn test_put_get_one() {
        let mut mc = MessageCache::new(10, 15);

        let topic1_hash = Topic::new("topic1".into()).no_hash().clone();
        let topic2_hash = Topic::new("topic2".into()).no_hash().clone();

        let m = gen_testm(10, vec![topic1_hash, topic2_hash]);

        mc.put(m.clone());

        assert!(mc.history[0].len() == 1);

        let fetched = mc.get(&m.id());

        assert_eq!(fetched.is_none(), false);
        assert_eq!(fetched.is_some(), true);

        // Make sure it is the same fetched message
        match fetched {
            Some(x) => assert_eq!(*x, m),
            _ => assert!(false),
        }
    }

    #[test]
    /// Test attempting to 'get' with a wrong id.
    fn test_get_wrong() {
        let mut mc = MessageCache::new(10, 15);

        let topic1_hash = Topic::new("topic1".into()).no_hash().clone();
        let topic2_hash = Topic::new("topic2".into()).no_hash().clone();

        let m = gen_testm(10, vec![topic1_hash, topic2_hash]);

        mc.put(m.clone());

        // Try to get an incorrect ID
        let wrong_id = String::from("wrongid");
        let fetched = mc.get(&wrong_id);
        assert_eq!(fetched.is_none(), true);
    }

    #[test]
    /// Test attempting to 'get' empty message cache.
    fn test_get_empty() {
        let mc = MessageCache::new(10, 15);

        // Try to get an incorrect ID
        let wrong_string = String::from("imempty");
        let fetched = mc.get(&wrong_string);
        assert_eq!(fetched.is_none(), true);
    }

    #[test]
    /// Test adding a message with no topics.
    fn test_no_topic_put() {
        let mut mc = MessageCache::new(3, 5);

        // Build the message
        let m = gen_testm(1, vec![]);
        mc.put(m.clone());

        let fetched = mc.get(&m.id());

        // Make sure it is the same fetched message
        match fetched {
            Some(x) => assert_eq!(*x, m),
            _ => assert!(false),
        }
    }

    #[test]
    /// Test shift mechanism.
    fn test_shift() {
        let mut mc = MessageCache::new(1, 5);

        let topic1_hash = Topic::new("topic1".into()).no_hash().clone();
        let topic2_hash = Topic::new("topic2".into()).no_hash().clone();

        // Build the message
        for i in 0..10 {
            let m = gen_testm(i, vec![topic1_hash.clone(), topic2_hash.clone()]);
            mc.put(m.clone());
        }

        mc.shift();

        // Ensure the shift occurred
        assert!(mc.history[0].len() == 0);
        assert!(mc.history[1].len() == 10);

        // Make sure no messages deleted
        assert!(mc.msgs.len() == 10);
    }

    #[test]
    /// Test Shift with no additions.
    fn test_empty_shift() {
        let mut mc = MessageCache::new(1, 5);

        let topic1_hash = Topic::new("topic1".into()).no_hash().clone();
        let topic2_hash = Topic::new("topic2".into()).no_hash().clone();
        // Build the message
        for i in 0..10 {
            let m = gen_testm(i, vec![topic1_hash.clone(), topic2_hash.clone()]);
            mc.put(m.clone());
        }

        mc.shift();

        // Ensure the shift occurred
        assert!(mc.history[0].len() == 0);
        assert!(mc.history[1].len() == 10);

        mc.shift();

        assert!(mc.history[2].len() == 10);
        assert!(mc.history[1].len() == 0);
        assert!(mc.history[0].len() == 0);
    }

    #[test]
    /// Test shift to see if the last history messages are removed.
    fn test_remove_last_from_shift() {
        let mut mc = MessageCache::new(4, 5);

        let topic1_hash = Topic::new("topic1".into()).no_hash().clone();
        let topic2_hash = Topic::new("topic2".into()).no_hash().clone();
        // Build the message
        for i in 0..10 {
            let m = gen_testm(i, vec![topic1_hash.clone(), topic2_hash.clone()]);
            mc.put(m.clone());
        }

        // Shift right until deleting messages
        mc.shift();
        mc.shift();
        mc.shift();
        mc.shift();

        assert_eq!(mc.history[mc.history.len() - 1].len(), 10);

        // Shift and delete the messages
        mc.shift();
        assert_eq!(mc.history[mc.history.len() - 1].len(), 0);
        assert_eq!(mc.history[0].len(), 0);
        assert_eq!(mc.msgs.len(), 0);
    }
}
