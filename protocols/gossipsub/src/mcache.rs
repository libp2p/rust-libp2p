use message::{MsgMap, MsgRep, GMessage, MsgHash, MsgId};

use std::collections::hash_map::HashMap;

use libp2p_floodsub::TopicMap;

/// The message cache used to track recently seen messages. FMI see
/// https://github.com/libp2p/specs/tree/master/pubsub/gossipsub#router-state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
        let m_hash = MsgHash::new(m);
        let msg_rep = MsgRep::hash(m_hash);
        self.msgs.insert(msg_rep, m);
    }

    pub fn put_with_msg_id_key(&mut self, m: GMessage) {
        let m_id = m.get_id().expect("The message was not published with an ID, use put_with_msg_hash_key instead");
        // let m_id = MsgId::new(m);
        let msg_rep = MsgRep::id(m_id);
        self.msgs.insert(msg_rep, m);
    }

    pub fn get()
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
    topics: TopicMap,
}
