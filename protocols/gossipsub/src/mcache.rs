use {TopicMap, TopicHash};
use constants::{GOSSIP_HIST_LEN, HISTORY_GOSSIP, SEEN_MSGS_CACHE};
use message::{MsgMap, GMessage, MsgHash};

use std::{
    collections::hash_map::Keys,
    fmt,
    iter,
    time::Duration,
};

use lru_time_cache::LruCache;

/// The message cache used to track recently seen messages. FMI see
/// https://github.com/libp2p/specs/tree/master/pubsub/gossipsub#router-state.
/// MCache is used in `ControlIHave`.
// Debug, Default, PartialEq, Eq can't be derived for LruCache.
#[derive(Clone)]
pub struct MCache {
    msgs: MsgMap,
    history: Vec<Vec<CacheEntry>>,
    seen: LruCache<MsgHash, CacheEntry>,
}

// Doesn't work, seen doesn't implement debug
// impl fmt::Debug for MCache {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         write!(f, "MCache {{ msgs: {:?}, history: {:?}, seen: {:?} }}",
//             self.msgs, self.history, self.seen)
//     }
// }

// All fields don't implement display.
// impl fmt::Display for MCache {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         write!(f, "MCache {{ msgs: {}, history: {}, seen: {} }}",
//             self.msgs, self.history, self.seen)
//     }
// }

impl MCache {
    pub fn new() -> Self {
        let time_to_live = Duration::from_secs(SEEN_MSGS_CACHE as u64);
        MCache {
            msgs: MsgMap::new(),
            history: Vec::with_capacity(GOSSIP_HIST_LEN as usize),
            seen: LruCache::<MsgHash, CacheEntry>::with_expiry_duration(
                time_to_live)
        }
    }

    /// Adds a message to the cache after constructing a `MsgHash` to use as a
    /// key.
    pub fn put(&mut self, m: GMessage) {
        self.put_many(iter::once(m))
    }

    /// Adds many messages to the cache, passing the messages and optionally
    /// the corresponding `MsgHash`es, in the same order.
    pub fn put_many(&mut self, msgs: impl IntoIterator<Item = GMessage>) {
        for m in msgs {
            let m_hash = MsgHash::new(m.clone());
            self.msgs.insert(m_hash.clone(), m.clone());
            self.history[0].push(CacheEntry { m_hash: m_hash,
                topics: m.clone().get_topic_map() })
        }
    }

    /// Retrieves a message from the cache by its ID, if it is still present.
    pub fn get(&self, mh: MsgHash) -> Option<&GMessage> {
        self.msgs.get(&mh)
    }

    /// Gets all the `MsgHash`es in self.msgs.
    pub fn msgs_keys(&self) -> Keys<MsgHash, GMessage> {
        self.msgs.keys()
    }

    /// Gets the `MsgHash`es in the history window, up to the `HISTORY_GOSSIP`
    /// index.
    // Due to the spec missing details, there is a lot of similarity here to
    // https://github.com/libp2p/go-libp2p-pubsub/blob/master/mcache.go#L37.
    // I haven't given much thought to whether there is a more idiomatic or
    // better way to write this.
    pub fn get_recent_msg_hashes_in_hist(&self, t_hash: TopicHash)
        -> Vec<MsgHash> {
        let mut m_hashes = Vec::new();
        for entries in &self.history[..(HISTORY_GOSSIP as usize)] {
            for entry in entries {
                for (th, _) in entry.clone().topics {
                    if th == t_hash {
                        m_hashes.push(entry.m_hash.clone());
                        break;
                    }
                }
            }
        }
        m_hashes
    }

    /// Retrieves all of the message IDs for messages in the current history
    /// window.
    // pub fn window(&self) -> Vec<MsgHash> {
    //     self.history.iter().map(|h| h.m_hash).collect()
    // }

    /// Shifts the current window, discarding messages older than the history
    /// length of the cache.
    pub fn shift(&mut self) {
        let hist = &mut self.history;
        let last = &hist[hist.len()-1];
        for entry in last {
            self.msgs.remove(&entry.m_hash);
        }
        hist.clone().insert(0, Vec::new());
    }

    pub fn get_seen(&self) -> &LruCache<MsgHash, CacheEntry> {
        &self.seen
    }
    // Consulted https://github.com/libp2p/go-libp2p-pubsub/blob/master/mcache.go
    // as https://github.com/libp2p/specs/tree/master/pubsub/gossipsub
    // is vague.
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    m_hash: MsgHash,
    topics: TopicMap,
}

impl CacheEntry {
}
