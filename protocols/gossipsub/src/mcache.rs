extern crate fnv;

use super::rpc_proto::Message;
use fnv::FnvHashMap;

/// CacheEntry stored in the history
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheEntry {
    mid: String,
    topics: Vec<String>,
}

/// MessageCache struct holding history of messages
#[derive(Debug, Clone, PartialEq)]
pub struct MessageCache {
    msgs: FnvHashMap<String, Message>,
    history: Vec<Vec<CacheEntry>>,
    gossip: usize,
}

/// Implementation of the MessageCache
impl MessageCache {
    pub fn new(gossip: usize, history_capacity: usize) -> MessageCache {
        MessageCache {
            gossip,
            msgs: FnvHashMap::default(),
            history: vec![Vec::new(); history_capacity],
        }
    }

    /// Put a message into the memory cache
    pub fn put(&mut self, msg: Message) -> Result<(), MsgError> {
        let message_id = msg_id(&msg)?;
        let cache_entry = CacheEntry {
            mid: message_id.clone(),
            topics: msg.get_topicIDs().to_vec(),
        };

        self.msgs.insert(message_id, msg);

        self.history[0].push(cache_entry);
        Ok(())
    }

    /// Get a message with `message_id`
    pub fn get(&self, message_id: &str) -> Option<&Message> {
        self.msgs.get(message_id)
    }

    /// Get a list of GossipIds for a given topic
    pub fn get_gossip_ids(&self, topic: &str) -> Vec<String> {
        self.history[..self.gossip]
            .iter()
            .fold(vec![], |mut current_entries, entries| {
                // search for entries with desired topic
                let mut found_entries: Vec<String> = entries
                    .iter()
                    .filter_map(|entry| {
                        if entry.topics.iter().any(|t| *t == topic) {
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
        let last_index = self.history.len() - 1;
        for entry in &self.history[last_index] {
            self.msgs.remove(&entry.mid);
        }

        // Pop the last value
        self.history.pop();

        // Insert an empty vec in position 0
        self.history.insert(0, Vec::new());

        // TODO bench which one is quicker
        // for i in (0..(self.history.len() - 1)).rev() {
        //     self.history[i+1] = self.history[i].clone();
        // }
        // self.history[0] = Vec::new();
    }
}

// Functions to be refactored later
/// Gets a unique message id.
/// Returns an error if the message has non-utf from or seqno values
fn msg_id(pmsg: &Message) -> Result<String, MsgError> {
    let from = String::from_utf8(pmsg.get_from().to_vec()).or(Err(MsgError::InvalidMessage))?;
    let seqno = String::from_utf8(pmsg.get_seqno().to_vec()).or(Err(MsgError::InvalidMessage))?;
    Ok(from + &seqno)
}

#[derive(Debug)]
pub enum MsgError {
    InvalidMessage,
}

#[cfg(test)]
mod tests {
    use super::super::protobuf;
    use super::*;

    fn gen_testm(x: usize, topics: Vec<String>) -> Message {
        let u8x: u8 = x as u8;
        let from: Vec<u8> = vec![u8x];
        let data: Vec<u8> = vec![u8x];
        let seqno: Vec<u8> = vec![u8x];
        let mut tids = protobuf::RepeatedField::new();

        for topic in topics {
            tids.push(topic);
        }

        let mut m = Message::new();
        m.set_from(from.clone());
        m.set_data(data);
        m.set_seqno(seqno);
        m.set_topicIDs(tids);

        m
    }

    #[test]
    /// Test that the message cache can be created
    fn test_new_cache() {
        let x: usize = 3;
        let mc = MessageCache::new(x, 5);

        assert_eq!(mc.gossip, x);
    }

    #[test]
    /// Test you can put one message and get one
    fn test_put_get_one() {
        let mut mc = MessageCache::new(10, 15);

        let m = gen_testm(
            10 as usize,
            vec![String::from("hello"), String::from("world")],
        );

        let res = mc.put(m.clone());
        assert_eq!(res.is_err(), false);
        assert_eq!(res.ok(), Some(()));

        assert!(mc.history[0].len() == 1);

        let mid = msg_id(&m.clone());
        assert_eq!(mid.is_err(), false);

        let fetched = match mid.ok() {
            Some(id) => mc.get(&id),
            _ => None,
        };

        assert_eq!(fetched.is_none(), false);
        assert_eq!(fetched.is_some(), true);

        // Make sure it is the same fetched message
        match fetched {
            Some(x) => assert_eq!(*x, m),
            _ => assert!(false),
        }
    }

    #[test]
    /// Test attempting to 'get' with a wrong id
    fn test_get_wrong() {
        let mut mc = MessageCache::new(10, 15);

        // Build the message
        let m = gen_testm(
            1 as usize,
            vec![String::from("hello"), String::from("world")],
        );

        let res = mc.put(m.clone());
        assert_eq!(res.is_err(), false);
        assert_eq!(res.ok(), Some(()));

        let mid = msg_id(&m.clone());
        assert_eq!(mid.is_err(), false);

        // Try to get an incorrect ID
        let wrong_string = String::from("wrongid");
        let fetched = mc.get(&wrong_string);
        assert_eq!(fetched.is_none(), true);
    }

    #[test]
    /// Test attempting to 'get' empty message cache
    fn test_get_empty() {
        let mc = MessageCache::new(10, 15);

        // Try to get an incorrect ID
        let wrong_string = String::from("imempty");
        let fetched = mc.get(&wrong_string);
        assert_eq!(fetched.is_none(), true);
    }

    #[test]
    /// Test adding a message with no topics
    fn test_no_topic_put() {
        let mut mc = MessageCache::new(3, 5);

        // Build the message
        let m = gen_testm(1 as usize, vec![]);

        let res = mc.put(m.clone());
        assert_eq!(res.is_err(), false);
        assert_eq!(res.ok(), Some(()));

        let mid = msg_id(&m.clone());
        let fetched = match mid.ok() {
            Some(id) => mc.get(&id),
            _ => None,
        };

        // Make sure it is the same fetched message
        match fetched {
            Some(x) => assert_eq!(*x, m),
            _ => assert!(false),
        }
    }

    #[test]
    /// Test shift mechanism
    fn test_shift() {
        let mut mc = MessageCache::new(1, 5);

        // Build the message
        for i in 0..10 {
            let m = gen_testm(
                i as usize,
                vec![String::from("hello"), String::from("world")],
            );
            let res = mc.put(m.clone());
            assert_eq!(res.is_err(), false);
        }

        mc.shift();

        // Ensure the shift occurred
        assert!(mc.history[0].len() == 0);
        assert!(mc.history[1].len() == 10);

        // Make sure no messages deleted
        assert!(mc.msgs.len() == 10);
    }

    #[test]
    /// Test Shift with no additions
    fn test_empty_shift() {
        let mut mc = MessageCache::new(1, 5);

        // Build the message
        for i in 0..10 {
            let m = gen_testm(
                i as usize,
                vec![String::from("hello"), String::from("world")],
            );
            let res = mc.put(m.clone());
            assert_eq!(res.is_err(), false);
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
    /// Test shift to see if the last history messages are removed
    fn test_remove_last_from_shift() {
        let mut mc = MessageCache::new(4, 5);

        for i in 0..10 {
            let m = gen_testm(
                i as usize,
                vec![String::from("hello"), String::from("world")],
            );
            let res = mc.put(m.clone());
            assert_eq!(res.is_err(), false);
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
