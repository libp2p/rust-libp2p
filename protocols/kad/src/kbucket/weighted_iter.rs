/*
 * Copyright 2020 Fluence Labs Limited
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::kbucket::{
    BucketIndex, ClosestBucketsIter, Distance, KBucketsTable, KeyBytes, Node, NodeStatus,
};

#[derive(Copy, Clone, Debug)]
struct Progress {
    need: usize,
    got: usize,
}

#[derive(Copy, Clone, Debug)]
enum State {
    Nowhere,
    Weighted(Progress),
    Swamp {
        /// After taking one element from swamp, go back to weighted, keeping saved progress
        saved: Option<Progress>,
    },
    Empty,
}

pub struct WeightedIter<'a, TKey, TVal> {
    target: &'a KeyBytes,
    start: BucketIndex,
    weighted_buckets: ClosestBucketsIter,
    weighted_iter: Option<DynIter<'a, TKey, TVal>>,
    swamp_buckets: ClosestBucketsIter,
    swamp_iter: Option<DynIter<'a, TKey, TVal>>,
    table: &'a KBucketsTable<TKey, TVal>, // TODO: make table &mut and call apply_pending?
    state: State,
}

type Entry<'a, TKey, TVal> = (&'a Node<TKey, TVal>, NodeStatus);
type DynIter<'a, TKey, TVal> = Box<dyn Iterator<Item = (&'a Node<TKey, TVal>, NodeStatus)> + 'a>;

impl<'a, TKey, TVal> WeightedIter<'a, TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    pub fn new(
        table: &'a KBucketsTable<TKey, TVal>,
        distance: Distance,
        target: &'a KeyBytes,
    ) -> impl Iterator<Item = Entry<'a, TKey, TVal>> + 'a
    where
        TKey: Clone + AsRef<KeyBytes>,
        TVal: Clone,
    {
        let start = BucketIndex::new(&distance).unwrap_or(BucketIndex(0));
        WeightedIter {
            target,
            start: start.clone(),
            weighted_buckets: ClosestBucketsIter::new(distance),
            weighted_iter: None,
            swamp_buckets: ClosestBucketsIter::new(distance),
            swamp_iter: None,
            table,
            state: State::Nowhere,
        }
    }

    pub fn num_weighted(&self, idx: BucketIndex) -> usize {
        let distance = (self.start.get() as isize - idx.get() as isize).abs();
        // After distance > 2, result will always be equal to 1
        let pow = (distance + 2).min(4);
        // 16/2^(distance+2)
        16 / 2usize.pow(pow as u32) // TODO: check overflows?
    }

    // Take iterator for next weighted bucket, save it to self.weighted_iter, and return
    pub fn next_weighted_bucket(&mut self) -> Option<(BucketIndex, &mut DynIter<'a, TKey, TVal>)> {
        let idx = self.weighted_buckets.next()?;
        let bucket = self.table.buckets.get(idx.get());
        let v = bucket.map(|b| self.sort_bucket(b.weighted().collect::<Vec<_>>()));
        self.weighted_iter = v;
        // self.weighted_iter = bucket.map(|b| Box::new(b.weighted()) as _);
        self.weighted_iter.as_mut().map(|iter| (idx, iter))
    }

    // Take next weighted element, return None when there's no weighted elements
    pub fn next_weighted(&mut self) -> Option<Entry<'a, TKey, TVal>> {
        // Take current weighted_iter or the next one
        if let Some(iter) = self.weighted_iter.as_deref_mut() {
            return iter.next();
        }

        let iter = self.next_weighted_bucket().map(|(_, i)| i)?;
        iter.next()
    }

    pub fn next_swamp_bucket(&mut self) -> Option<&'_ mut DynIter<'a, TKey, TVal>> {
        let idx = self.swamp_buckets.next()?;
        let bucket = self.table.buckets.get(idx.get());
        let v = bucket.map(|b| self.sort_bucket(b.swamp().collect::<Vec<_>>()));
        self.swamp_iter = v;
        self.swamp_iter.as_mut()
    }

    pub fn next_swamp(&mut self) -> Option<Entry<'a, TKey, TVal>> {
        if let Some(iter) = self.swamp_iter.as_deref_mut() {
            return iter.next();
        }

        let iter = self.next_swamp_bucket()?;
        iter.next()
    }

    pub fn sort_bucket(&self, mut bucket: Vec<Entry<'a, TKey, TVal>>) -> DynIter<'a, TKey, TVal> {
        bucket.sort_by_cached_key(|(e, _)| self.target.distance(e.key.as_ref()));

        Box::new(bucket.into_iter())
    }
}

impl<'a, TKey, TVal> Iterator for WeightedIter<'a, TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    type Item = Entry<'a, TKey, TVal>;

    fn next(&mut self) -> Option<Self::Item> {
        use State::*;

        loop {
            let (state, result) = match self.state {
                // Not yet started, or just finished a bucket
                // Here we decide where to go next
                Nowhere => {
                    // If there is a weighted bucket, try take element from it
                    if let Some((idx, iter)) = self.next_weighted_bucket() {
                        if let Some(elem) = iter.next() {
                            // Found weighted element
                            let state = Weighted(Progress {
                                got: 1,
                                need: self.num_weighted(idx),
                            });
                            (state, Some(elem))
                        } else {
                            // Weighted bucket was empty: go decide again
                            (Nowhere, None)
                        }
                    } else {
                        // There are no weighted buckets, go to swamp
                        (Swamp { saved: None }, None)
                    }
                }
                // Iterating through a weighted bucket, need more elements
                Weighted(Progress { got, need }) if got < need => {
                    if let Some(elem) = self.next_weighted() {
                        // Found weighted element, go take more
                        let state = Weighted(Progress { got: got + 1, need });
                        (state, Some(elem))
                    } else {
                        // Current bucket is empty, we're nowhere: need to decide where to go next
                        (Nowhere, None)
                    }
                }
                // Got enough weighted, go to swamp (saving progress, to return back with it)
                Weighted(Progress { need, .. }) => {
                    (
                        Swamp {
                            // Set 'got' to zero, so when we get element from swarm, we start afresh
                            saved: Some(Progress { need, got: 0 }),
                        },
                        None,
                    )
                }
                // Take one element from swamp, and go to Weighted
                Swamp { saved: Some(saved) } => {
                    if let Some(elem) = self.next_swamp() {
                        // We always take just a single element from the swamp
                        // And then go back to weighted
                        (Weighted(saved), Some(elem))
                    } else if self.next_swamp_bucket().is_some() {
                        // Current bucket was empty, take next one
                        (Swamp { saved: Some(saved) }, None)
                    } else {
                        // No more swamp buckets, go drain weighted
                        (Weighted(saved), None)
                    }
                }
                // Weighted buckets are empty
                Swamp { saved } => {
                    if let Some(elem) = self.next_swamp() {
                        // Keep draining bucket until it's empty
                        (Swamp { saved }, Some(elem))
                    } else if self.next_swamp_bucket().is_some() {
                        // Current bucket was empty, go try next one
                        (Swamp { saved }, None)
                    } else {
                        // Routing table is empty
                        (Empty, None)
                    }
                }
                Empty => (Empty, None),
            };

            self.state = state;

            if result.is_some() {
                return result;
            }

            if let Empty = &self.state {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use libp2p_core::identity::ed25519;
    use libp2p_core::{identity, PeerId};

    use crate::kbucket::{Entry, InsertResult, Key};

    use super::*;

    #[test]
    /// Insert:
    /// 1 weighted far away
    /// lots of swamp near
    ///
    /// Expect:
    /// weighted still in the results
    fn weighted_first() {
        env_logger::builder()
            .filter_level(log::LevelFilter::Debug)
            .try_init()
            .ok();

        let keypair = ed25519::Keypair::generate();
        let public_key = identity::PublicKey::Ed25519(keypair.public());
        let local_key = Key::from(PeerId::from(public_key));

        let mut table =
            KBucketsTable::<_, ()>::new(keypair, local_key.clone(), Duration::from_secs(5));

        let mut insert = |id: Key<PeerId>, weight: u32| {
            if let Entry::Absent(e) = table.entry(&id) {
                if let InsertResult::Inserted = e.insert((), NodeStatus::Connected, weight) {
                    return Some(id);
                }
            }
            None
        };

        let target_min_distance = 250;
        // Generate weighted target
        let target = loop {
            let id = Key::from(PeerId::random());
            let distance = local_key.distance(&id);
            let idx = BucketIndex::new(&distance).unwrap_or(BucketIndex(0)).get();
            if idx > target_min_distance {
                let target = insert(id, 10).expect("inserted");
                break target;
            }
        };

        let mut swamp = 100;
        let max_distance = 252;
        while swamp > 0 {
            let id = Key::from(PeerId::random());
            let distance = local_key.distance(&id);
            let idx = BucketIndex::new(&distance).unwrap_or(BucketIndex(0)).get();
            if idx < max_distance && insert(id, 0).is_some() {
                swamp -= 1;
            }
        }

        // Start from bucket 0
        let closest = table.closest_keys(&local_key).take(1).collect::<Vec<_>>();
        assert!(closest.contains(&target));

        // Start from target
        let closest = table.closest_keys(&target).take(1).collect::<Vec<_>>();
        assert!(closest.contains(&target));

        // Start from random
        let random = Key::from(PeerId::random());
        let closest = table.closest_keys(&random).take(1).collect::<Vec<_>>();
        assert!(closest.contains(&target));
    }
}
