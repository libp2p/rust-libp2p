use crate::query::Query;
use crate::QueryId;
use fnv::FnvHashMap;
use std::collections::VecDeque;

/// Wrapper data structure that guarantees fairness of the pending queries by maintaining insertion
/// order in addition to the hashmap. In order to take advantage of this [`PendingQueries::next()`]
/// method must be used.
pub(super) struct PendingQueries<TInner> {
    // IDs corresponding to `queries`, though for efficiency purposes there might be entries
    // resent for queries that were already removed.
    query_ids: VecDeque<QueryId>,
    queries: FnvHashMap<QueryId, Query<TInner>>,
}

impl<TInner> Default for PendingQueries<TInner> {
    fn default() -> Self {
        Self {
            query_ids: Default::default(),
            queries: Default::default(),
        }
    }
}

impl<TInner> PendingQueries<TInner> {
    pub(super) fn contains_key(&self, id: &QueryId) -> bool {
        self.queries.contains_key(id)
    }

    pub(super) fn insert(&mut self, id: QueryId, query: Query<TInner>) -> Option<Query<TInner>> {
        self.query_ids.push_back(id);
        self.queries.insert(id, query)
    }

    pub(super) fn get(&self, id: &QueryId) -> Option<&Query<TInner>> {
        self.queries.get(id)
    }

    pub(super) fn get_mut(&mut self, id: &QueryId) -> Option<&mut Query<TInner>> {
        self.queries.get_mut(id)
    }

    pub(super) fn remove(&mut self, id: &QueryId) -> Option<Query<TInner>> {
        // We only remove IDs from the front and back, the rest can remain for some time
        if self.query_ids.front() == Some(id) {
            self.query_ids.pop_front();
        }
        if self.query_ids.back() == Some(id) {
            self.query_ids.pop_back();
        }

        self.queries.remove(id)
    }

    pub(super) fn len(&self) -> usize {
        self.queries.len()
    }

    /// Get next pending query in insertion order.
    pub(super) fn next(&mut self) -> Option<Query<TInner>> {
        while let Some(query) = self.query_ids.pop_front() {
            // Query may not exist anymore, check for this.
            if let Some(query) = self.queries.remove(&query) {
                return Some(query);
            }
        }

        None
    }

    /// Order of items in this iterator is not guaranteed.
    pub(super) fn values(&self) -> impl Iterator<Item = &Query<TInner>> {
        self.queries.values()
    }

    /// Order of items in this iterator is not guaranteed.
    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut Query<TInner>> {
        self.queries.values_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::PendingQueries;
    use crate::query::{FixedPeersIter, Query, QueryId, QueryPeerIter, QueryStats};
    use std::num::NonZeroUsize;

    #[test]
    fn basic() {
        let mut ids = (0..100).map(QueryId).collect::<Vec<_>>();
        let mut pending_queries = PendingQueries::<()>::default();
        for &id in &ids {
            let old_query = pending_queries.insert(
                id,
                Query {
                    id,
                    peer_iter: QueryPeerIter::Fixed(FixedPeersIter::new(
                        [],
                        NonZeroUsize::new(1).unwrap(),
                    )),
                    stats: QueryStats::empty(),
                    inner: (),
                },
            );

            assert!(old_query.is_none());
        }

        assert_eq!(pending_queries.len(), ids.len());

        {
            let values_keys = pending_queries
                .values()
                .map(|query| query.id())
                .collect::<Vec<_>>();
            let values_mut_keys = pending_queries
                .values_mut()
                .map(|query| query.id())
                .collect::<Vec<_>>();
            assert_eq!(values_keys, values_mut_keys);
            // Order is not guaranteed and must not match due to hashing
            assert_ne!(values_keys, ids);
            // Order is not guaranteed and must not match due to hashing
            assert_ne!(values_mut_keys, ids);
        }

        for id in &ids {
            assert!(pending_queries.contains_key(id));
            assert_eq!(&pending_queries.get(id).unwrap().id, id);
            assert_eq!(&pending_queries.get_mut(id).unwrap().id, id);
        }

        {
            let removed_id = ids.remove(ids.len() / 2);
            assert_eq!(pending_queries.remove(&removed_id).unwrap().id, removed_id);

            for id in ids {
                assert_eq!(pending_queries.next().unwrap().id, id);
            }
        }
    }
}
