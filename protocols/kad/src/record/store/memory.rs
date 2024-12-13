// Copyright 2019 Parity Technologies (UK) Ltd.
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

use std::{
    collections::{hash_map, hash_set, HashMap, HashSet},
    iter,
};

use smallvec::SmallVec;

use super::*;
use crate::kbucket;

/// In-memory implementation of a `RecordStore`.
pub struct MemoryStore {
    /// The identity of the peer owning the store.
    local_key: kbucket::Key<PeerId>,
    /// The configuration of the store.
    config: MemoryStoreConfig,
    /// The stored (regular) records.
    records: HashMap<Key, Record>,
    /// The stored provider records.
    providers: HashMap<Key, SmallVec<[ProviderRecord; K_VALUE.get()]>>,
    /// The set of all provider records for the node identified by `local_key`.
    ///
    /// Must be kept in sync with `providers`.
    provided: HashSet<ProviderRecord>,
}

/// Configuration for a `MemoryStore`.
#[derive(Debug, Clone)]
pub struct MemoryStoreConfig {
    /// The maximum number of records.
    pub max_records: usize,
    /// The maximum size of record values, in bytes.
    pub max_value_bytes: usize,
    /// The maximum number of providers stored for a key.
    ///
    /// This should match up with the chosen replication factor.
    pub max_providers_per_key: usize,
    /// The maximum number of provider records for which the
    /// local node is the provider.
    pub max_provided_keys: usize,
}

impl Default for MemoryStoreConfig {
    fn default() -> Self {
        Self {
            max_records: 1024,
            max_value_bytes: 65 * 1024,
            max_provided_keys: 1024,
            max_providers_per_key: K_VALUE.get(),
        }
    }
}

impl MemoryStore {
    /// Creates a new `MemoryRecordStore` with a default configuration.
    pub fn new(local_id: PeerId) -> Self {
        Self::with_config(local_id, Default::default())
    }

    /// Creates a new `MemoryRecordStore` with the given configuration.
    pub fn with_config(local_id: PeerId, config: MemoryStoreConfig) -> Self {
        MemoryStore {
            local_key: kbucket::Key::from(local_id),
            config,
            records: HashMap::default(),
            provided: HashSet::default(),
            providers: HashMap::default(),
        }
    }

    /// Retains the records satisfying a predicate.
    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&Key, &mut Record) -> bool,
    {
        self.records.retain(f);
    }
}

impl RecordStore for MemoryStore {
    type RecordsIter<'a> =
        iter::Map<hash_map::Values<'a, Key, Record>, fn(&'a Record) -> Cow<'a, Record>>;

    type ProvidedIter<'a> = iter::Map<
        hash_set::Iter<'a, ProviderRecord>,
        fn(&'a ProviderRecord) -> Cow<'a, ProviderRecord>,
    >;

    fn get(&self, k: &Key) -> Option<Cow<'_, Record>> {
        self.records.get(k).map(Cow::Borrowed)
    }

    fn put(&mut self, r: Record) -> Result<()> {
        if r.value.len() >= self.config.max_value_bytes {
            return Err(Error::ValueTooLarge);
        }

        let num_records = self.records.len();

        match self.records.entry(r.key.clone()) {
            hash_map::Entry::Occupied(mut e) => {
                e.insert(r);
            }
            hash_map::Entry::Vacant(e) => {
                if num_records >= self.config.max_records {
                    return Err(Error::MaxRecords);
                }
                e.insert(r);
            }
        }

        Ok(())
    }

    fn remove(&mut self, k: &Key) {
        self.records.remove(k);
    }

    fn records(&self) -> Self::RecordsIter<'_> {
        self.records.values().map(Cow::Borrowed)
    }

    fn add_provider(&mut self, record: ProviderRecord) -> Result<()> {
        let num_keys = self.providers.len();

        // Obtain the entry
        let providers = match self.providers.entry(record.key.clone()) {
            e @ hash_map::Entry::Occupied(_) => e,
            e @ hash_map::Entry::Vacant(_) => {
                if self.config.max_provided_keys == num_keys {
                    return Err(Error::MaxProvidedKeys);
                }
                e
            }
        }
        .or_insert_with(Default::default);

        for p in providers.iter_mut() {
            if p.provider == record.provider {
                // In-place update of an existing provider record.
                if self.local_key.preimage() == &record.provider {
                    self.provided.remove(p);
                    self.provided.insert(record.clone());
                }
                *p = record;
                return Ok(());
            }
        }

        // If the providers list is full, we ignore the new provider.
        // This strategy can mitigate Sybil attacks, in which an attacker
        // floods the network with fake provider records.
        if providers.len() == self.config.max_providers_per_key {
            return Ok(());
        }

        // Otherwise, insert the new provider record.
        if self.local_key.preimage() == &record.provider {
            self.provided.insert(record.clone());
        }
        providers.push(record);

        Ok(())
    }

    fn providers(&self, key: &Key) -> Vec<ProviderRecord> {
        self.providers
            .get(key)
            .map_or_else(Vec::new, |ps| ps.clone().into_vec())
    }

    fn provided(&self) -> Self::ProvidedIter<'_> {
        self.provided.iter().map(Cow::Borrowed)
    }

    fn remove_provider(&mut self, key: &Key, provider: &PeerId) {
        if let hash_map::Entry::Occupied(mut e) = self.providers.entry(key.clone()) {
            let providers = e.get_mut();
            if let Some(i) = providers.iter().position(|p| &p.provider == provider) {
                let p = providers.remove(i);
                if &p.provider == self.local_key.preimage() {
                    self.provided.remove(&p);
                }
            }
            if providers.is_empty() {
                e.remove();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::*;
    use rand::Rng;

    use super::*;
    use crate::SHA_256_MH;

    fn random_multihash() -> Multihash<64> {
        Multihash::wrap(SHA_256_MH, &rand::thread_rng().gen::<[u8; 32]>()).unwrap()
    }
    #[test]
    fn put_get_remove_record() {
        fn prop(r: Record) {
            let mut store = MemoryStore::new(PeerId::random());
            assert!(store.put(r.clone()).is_ok());
            assert_eq!(Some(Cow::Borrowed(&r)), store.get(&r.key));
            store.remove(&r.key);
            assert!(store.get(&r.key).is_none());
        }
        quickcheck(prop as fn(_))
    }

    #[test]
    fn add_get_remove_provider() {
        fn prop(r: ProviderRecord) {
            let mut store = MemoryStore::new(PeerId::random());
            assert!(store.add_provider(r.clone()).is_ok());
            assert!(store.providers(&r.key).contains(&r));
            store.remove_provider(&r.key, &r.provider);
            assert!(!store.providers(&r.key).contains(&r));
        }
        quickcheck(prop as fn(_))
    }

    #[test]
    fn provided() {
        let id = PeerId::random();
        let mut store = MemoryStore::new(id);
        let key = random_multihash();
        let rec = ProviderRecord::new(key, id, Vec::new());
        assert!(store.add_provider(rec.clone()).is_ok());
        assert_eq!(
            vec![Cow::Borrowed(&rec)],
            store.provided().collect::<Vec<_>>()
        );
        store.remove_provider(&rec.key, &id);
        assert_eq!(store.provided().count(), 0);
    }

    #[test]
    fn update_provider() {
        let mut store = MemoryStore::new(PeerId::random());
        let key = random_multihash();
        let prv = PeerId::random();
        let mut rec = ProviderRecord::new(key, prv, Vec::new());
        assert!(store.add_provider(rec.clone()).is_ok());
        assert_eq!(vec![rec.clone()], store.providers(&rec.key).to_vec());
        rec.expires = Some(Instant::now());
        assert!(store.add_provider(rec.clone()).is_ok());
        assert_eq!(vec![rec.clone()], store.providers(&rec.key).to_vec());
    }

    #[test]
    fn update_provided() {
        let prv = PeerId::random();
        let mut store = MemoryStore::new(prv);
        let key = random_multihash();
        let mut rec = ProviderRecord::new(key, prv, Vec::new());
        assert!(store.add_provider(rec.clone()).is_ok());
        assert_eq!(
            vec![Cow::Borrowed(&rec)],
            store.provided().collect::<Vec<_>>()
        );
        rec.expires = Some(Instant::now());
        assert!(store.add_provider(rec.clone()).is_ok());
        assert_eq!(
            vec![Cow::Borrowed(&rec)],
            store.provided().collect::<Vec<_>>()
        );
    }

    #[test]
    fn max_providers_per_key() {
        let config = MemoryStoreConfig::default();
        let key = kbucket::Key::new(Key::from(random_multihash()));

        let mut store = MemoryStore::with_config(PeerId::random(), config.clone());
        let peers = (0..config.max_providers_per_key)
            .map(|_| PeerId::random())
            .collect::<Vec<_>>();
        for peer in peers {
            let rec = ProviderRecord::new(key.preimage().clone(), peer, Vec::new());
            assert!(store.add_provider(rec).is_ok());
        }

        // The new provider cannot be added because the key is already saturated.
        let peer = PeerId::random();
        let rec = ProviderRecord::new(key.preimage().clone(), peer, Vec::new());
        assert!(store.add_provider(rec.clone()).is_ok());
        assert!(!store.providers(&rec.key).contains(&rec));
    }

    #[test]
    fn max_provided_keys() {
        let mut store = MemoryStore::new(PeerId::random());
        for _ in 0..store.config.max_provided_keys {
            let key = random_multihash();
            let prv = PeerId::random();
            let rec = ProviderRecord::new(key, prv, Vec::new());
            let _ = store.add_provider(rec);
        }
        let key = random_multihash();
        let prv = PeerId::random();
        let rec = ProviderRecord::new(key, prv, Vec::new());
        match store.add_provider(rec) {
            Err(Error::MaxProvidedKeys) => {}
            _ => panic!("Unexpected result"),
        }
    }
}
