use constants::GOSSIP_HIST_LEN;
use message::{MsgMap, GMessage, MsgHash};
use TopicMap;

use std::collections::hash_map::Keys;

/// The message cache used to track recently seen messages. FMI see
/// https://github.com/libp2p/specs/tree/master/pubsub/gossipsub#router-state.
/// MCache is used in `ControlIHave`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MCache {
    msgs: MsgMap,
    history: Vec<CacheEntry>,
}

impl MCache {
    pub fn new() -> Self {
        MCache {
            msgs: MsgMap::new(),
            history: Vec::with_capacity(GOSSIP_HIST_LEN as usize),
        }
    }

    /// Adds a message to the cache after constructing a `MsgHash` to use as a
    /// key.
    // TODO: should also add to the current window
    pub fn put(&mut self, m: GMessage) {
        let m_hash = MsgHash::new(m);
        self.msgs.insert(m_hash, m);
    }

    pub fn put_many_via_hashes(&mut self,
    mhs: impl IntoIterator<Item = impl Into<GMessage>>) {
        for mh in mhs.into_iter() {
            let m = mh.into();
            // TODO: This is potentially wasteful if `mh` is a `MsgHash`, but
            // I haven't put much thought into how to convert `mh` to a
            // `MsgHash`.
            let m_hash = MsgHash::new(m);
            self.msgs.insert(m_hash, m);
        }
    }

    // pub fn put_with_msg_id_key(&mut self, m: GMessage) {
    //     let m_id = m.get_id().expect("The message was not published with an \
    //         ID, use put_with_msg_hash_key instead");
    //     // let m_id = MsgId::new(m);
    //     let msg_rep = MsgRep::id(m_id);
    //     self.msgs.insert(msg_rep, m);
    // }

    /// Retrieves a message from the cache by its ID, if it is still present.
    pub fn get(&self, mh: MsgHash) -> Option<&GMessage> {
        self.msgs.get(&mh)
    }

    pub fn msgs_keys(&self) -> Keys<MsgHash, GMessage> {
        self.msgs.keys()
    }

    // TODO: methods for window, shift
    // mcache.window(): retrieves the message IDs for messages in the current history window.
    // mcache.shift(): shifts the current window, discarding messages older than the history length of the cache.
    // Consult https://github.com/libp2p/go-libp2p-pubsub/blob/master/mcache.go
    // as https://github.com/libp2p/specs/tree/master/pubsub/gossipsub
    // is vague.
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheEntry {
    m_hash: MsgHash,
    topics: TopicMap,
}
