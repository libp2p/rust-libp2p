use message::{MsgMap, MsgRep, GMessage, MsgHash};

use std::collections::hash_map::HashMap;

use libp2p_floodsub::{TopicMap, TopicIdMap, TopicHashMap};

/// The message cache used to track recently seen messages. FMI see
/// https://github.com/libp2p/specs/tree/master/pubsub/gossipsub#router-state.
#[derive(Debug, Clone)]
pub struct MCache {
    msgs: MsgMap,
    history: Vec<CacheEntry>,
}

impl MCache {
    pub fn new() -> Self {
        MCache {
            msgs: HashMap::new(),
            history: Vec::new(),
        }
    }

    /// Adds a message to the cache after constructing a `MsgHash` to use as a
    /// key.
    // TODO: should also add to the current window
    pub fn put_with_msg_hash_key(&mut self, m: GMessage) {
        let mhash = MsgHash::new(m);
        let msg_rep = MsgRep::hash(mhash);
        self.msgs.insert(msg_rep, m);
    }

    // TODO: methods for get, window, shift
    // mcache.get(id): retrieves a message from the cache by its ID, if it is still present.
    // mcache.window(): retrieves the message IDs for messages in the current history window.
    // mcache.shift(): shifts the current window, discarding messages older than the history length of the cache.
    // Consult https://github.com/libp2p/go-libp2p-pubsub/blob/master/mcache.go
    // as https://github.com/libp2p/specs/tree/master/pubsub/gossipsub
    // is vague.
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheEntry {
    msg_rep: MsgRep,
    topics: TopicIdMap,
}
