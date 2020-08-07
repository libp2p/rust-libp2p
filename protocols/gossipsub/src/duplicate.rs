///! This implements a time-based LRU cache for checking gossipsub message duplicates.
use fnv::FnvHashSet;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

struct Element<Key> {
    /// The key being inserted.
    key: Key,
    /// The instant the key was inserted.
    inserted: Instant,
}

pub struct DuplicateCache<Key> {
    /// The duplicate cache.
    map: FnvHashSet<Key>,
    /// An ordered list of keys by insert time.
    list: VecDeque<Element<Key>>,
    /// The time elements remain in the cache.
    ttl: Duration,
}

impl<Key> DuplicateCache<Key>
where
    Key: Eq + std::hash::Hash + Clone,
{
    pub fn new(ttl: Duration) -> Self {
        DuplicateCache {
            map: FnvHashSet::default(),
            list: VecDeque::new(),
            ttl,
        }
    }

    // Inserts new elements and removes any expired elements.
    //
    // If the key was not present this returns `true`. If the value was already present this
    // returns `false`.
    pub fn insert(&mut self, key: Key) -> bool {
        // check the cache before removing elements
        let result = self.map.insert(key.clone());

        let now = Instant::now();
        // add the new key to the list
        self.list.push_back(Element {
            key,
            inserted: now.clone(),
        });

        // remove any expired results
        while let Some(element) = self.list.pop_front() {
            if element.inserted + self.ttl > now {
                self.list.push_front(element);
                break;
            }
            self.map.remove(&element.key);
        }
        result
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn cache_added_entries_exist() {
        let mut cache = DuplicateCache::new(Duration::from_secs(10));

        cache.insert("t");
        cache.insert("e");

        // Should report that 't' and 't' already exists
        assert!(!cache.insert("t"));
        assert!(!cache.insert("e"));
    }

    #[test]
    fn cache_entries_expire() {
        let mut cache = DuplicateCache::new(Duration::from_millis(100));

        cache.insert("t");
        assert!(!cache.insert("t"));
        cache.insert("e");
        //assert!(!cache.insert("t"));
        assert!(!cache.insert("e"));
        // sleep until cache expiry
        std::thread::sleep(Duration::from_millis(101));
        // add another element to clear previous cache
        cache.insert("s");

        // should be removed from the cache
        assert!(cache.insert("t"));
    }
}
