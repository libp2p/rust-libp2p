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

//! Implementation of a Kademlia routing table as used by a single peer
//! participating in a Kademlia DHT.
//!
//! The entry point for the API of this module is a [`KBucketsTable`].
//!
//! ## Pending Insertions
//!
//! When the bucket associated with the `Key` of an inserted entry is full
//! but contains disconnected nodes, it accepts a [`PendingEntry`].
//! Pending entries are inserted lazily when their timeout is found to be expired
//! upon querying the `KBucketsTable`. When that happens, the `KBucketsTable` records
//! an [`AppliedPending`] result which must be consumed by calling [`take_applied_pending`]
//! regularly and / or after performing lookup operations like [`entry`] and [`closest`].
//!
//! [`entry`]: kbucket::KBucketsTable::entry
//! [`closest`]: kbucket::KBucketsTable::closest
//! [`AppliedPending`]: kbucket::AppliedPending
//! [`KBucketsTable`]: kbucket::KBucketsTable
//! [`take_applied_pending`]: kbucket::KBucketsTable::take_applied_pending
//! [`PendingEntry`]: kbucket::PendingEntry

// [Implementation Notes]
//
// 1. Routing Table Layout
//
// The routing table is currently implemented as a fixed-size "array" of
// buckets, ordered by increasing distance relative to a local key
// that identifies the local peer. This is an often-used, simplified
// implementation that approximates the properties of the b-tree (or prefix tree)
// implementation described in the full paper [0], whereby buckets are split on-demand.
// This should be treated as an implementation detail, however, so that the
// implementation may change in the future without breaking the API.
//
// 2. Replacement Cache
//
// In this implementation, the "replacement cache" for unresponsive peers
// consists of a single entry per bucket. Furthermore, this implementation is
// currently tailored to connection-oriented transports, meaning that the
// "LRU"-based ordering of entries in a bucket is actually based on the last reported
// connection status of the corresponding peers, from least-recently (dis)connected to
// most-recently (dis)connected, and controlled through the `Entry` API. As a result,
// the nodes in the buckets are not reordered as a result of RPC activity, but only as a
// result of nodes being marked as connected or disconnected. In particular,
// if a bucket is full and contains only entries for peers that are considered
// connected, no pending entry is accepted. See the `bucket` submodule for
// further details.
//
// [0]: https://pdos.csail.mit.edu/~petar/papers/maymounkov-kademlia-lncs.pdf

mod bucket;
mod entry;
mod key;

pub use entry::*;

use arrayvec::{self, ArrayVec};
use bucket::KBucket;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Maximum number of k-buckets.
const NUM_BUCKETS: usize = 256;

/// A `KBucketsTable` represents a Kademlia routing table.
#[derive(Debug, Clone)]
pub struct KBucketsTable<TPeerId, TVal> {
    /// The key identifying the local peer that owns the routing table.
    local_key: Key<TPeerId>,
    /// The buckets comprising the routing table.
    buckets: Vec<KBucket<TPeerId, TVal>>,
    /// The list of evicted entries that have been replaced with pending
    /// entries since the last call to [`KBucketsTable::take_applied_pending`].
    applied_pending: VecDeque<AppliedPending<TPeerId, TVal>>
}

/// A (type-safe) index into a `KBucketsTable`, i.e. a non-negative integer in the
/// interval `[0, NUM_BUCKETS)`.
#[derive(Copy, Clone, PartialEq, Eq)]
struct BucketIndex(usize);

impl BucketIndex {
    /// Creates a new `BucketIndex` for a `Distance`.
    ///
    /// The given distance is interpreted as the distance from a `local_key` of
    /// a `KBucketsTable`. If the distance is zero, `None` is returned, in
    /// recognition of the fact that the only key with distance `0` to a
    /// `local_key` is the `local_key` itself, which does not belong in any
    /// bucket.
    fn new(d: &Distance) -> Option<BucketIndex> {
        (NUM_BUCKETS - d.0.leading_zeros() as usize)
            .checked_sub(1)
            .map(BucketIndex)
    }

    /// Gets the index value as an unsigned integer.
    fn get(&self) -> usize {
        self.0
    }

    /// Generates a random distance that falls into the bucket for this index.
    fn rand_distance(&self, rng: &mut impl rand::Rng) -> Distance {
        let mut bytes = [0u8; 32];
        let quot = self.0 / 8;
        for i in 0 .. quot {
            bytes[31 - i] = rng.gen();
        }
        let rem = self.0 % 8;
        let lower = usize::pow(2, rem as u32);
        let upper = usize::pow(2, rem as u32 + 1);
        bytes[31 - quot] = rng.gen_range(lower, upper) as u8;
        Distance(bigint::U256::from(bytes))
    }
}

impl<TPeerId, TVal> KBucketsTable<TPeerId, TVal>
where
    TPeerId: Clone,
{
    /// Creates a new, empty Kademlia routing table with entries partitioned
    /// into buckets as per the Kademlia protocol.
    ///
    /// The given `pending_timeout` specifies the duration after creation of
    /// a [`PendingEntry`] after which it becomes eligible for insertion into
    /// a full bucket, replacing the least-recently (dis)connected node.
    pub fn new(local_key: Key<TPeerId>, pending_timeout: Duration) -> Self {
        KBucketsTable {
            local_key,
            buckets: (0 .. NUM_BUCKETS).map(|_| KBucket::new(pending_timeout)).collect(),
            applied_pending: VecDeque::new()
        }
    }

    /// Returns the local key.
    pub fn local_key(&self) -> &Key<TPeerId> {
        &self.local_key
    }

    /// Returns an `Entry` for the given key, representing the state of the entry
    /// in the routing table.
    pub fn entry<'a>(&'a mut self, key: &'a Key<TPeerId>) -> Entry<'a, TPeerId, TVal> {
        let index = BucketIndex::new(&self.local_key.distance(key));
        if let Some(i) = index {
            let bucket = &mut self.buckets[i.get()];
            if let Some(applied) = bucket.apply_pending() {
                self.applied_pending.push_back(applied)
            }
            Entry::new(bucket, key)
        } else {
            Entry::SelfEntry
        }
    }

    /// Returns an iterator over all the entries in the routing table.
    pub fn iter<'a>(&'a mut self) -> impl Iterator<Item = EntryRefView<'a, TPeerId, TVal>> {
        let applied_pending = &mut self.applied_pending;
        self.buckets.iter_mut().flat_map(move |table| {
            if let Some(applied) = table.apply_pending() {
                applied_pending.push_back(applied)
            }
            let table = &*table;
            table.iter().map(move |(n, status)| {
                EntryRefView {
                    node: NodeRefView {
                        key: &n.key,
                        value: &n.value
                    },
                    status
                }
            })
        })
    }

    /// Returns a by-reference iterator over all buckets.
    ///
    /// The buckets are ordered by proximity to the `local_key`, i.e. the first
    /// bucket is the closest bucket (containing at most one key).
    pub fn buckets<'a>(&'a mut self) -> impl Iterator<Item = KBucketRef<'a, TPeerId, TVal>> + 'a {
        let applied_pending = &mut self.applied_pending;
        self.buckets.iter_mut().enumerate().map(move |(i, b)| {
            if let Some(applied) = b.apply_pending() {
                applied_pending.push_back(applied)
            }
            KBucketRef {
                index: BucketIndex(i),
                bucket: b
            }
        })
    }

    /// Consumes the next applied pending entry, if any.
    ///
    /// When an entry is attempted to be inserted and the respective bucket is full,
    /// it may be recorded as pending insertion after a timeout, see [`InsertResult::Pending`].
    ///
    /// If the oldest currently disconnected entry in the respective bucket does not change
    /// its status until the timeout of pending entry expires, it is evicted and
    /// the pending entry inserted instead. These insertions of pending entries
    /// happens lazily, whenever the `KBucketsTable` is accessed, and the corresponding
    /// buckets are updated accordingly. The fact that a pending entry was applied is
    /// recorded in the `KBucketsTable` in the form of `AppliedPending` results, which must be
    /// consumed by calling this function.
    pub fn take_applied_pending(&mut self) -> Option<AppliedPending<TPeerId, TVal>> {
        self.applied_pending.pop_front()
    }

    /// Returns an iterator over the keys closest to `target`, ordered by
    /// increasing distance.
    pub fn closest_keys<'a, T>(&'a mut self, target: &'a Key<T>)
        -> impl Iterator<Item = Key<TPeerId>> + 'a
    where
        T: Clone
    {
        let distance = self.local_key.distance(target);
        ClosestIter {
            target,
            iter: None,
            table: self,
            buckets_iter: ClosestBucketsIter::new(distance),
            fmap: |b: &KBucket<_, _>| -> ArrayVec<_> {
                b.iter().map(|(n,_)| n.key.clone()).collect()
            }
        }
    }

    /// Returns an iterator over the nodes closest to the `target` key, ordered by
    /// increasing distance.
    pub fn closest<'a, T>(&'a mut self, target: &'a Key<T>)
        -> impl Iterator<Item = EntryView<TPeerId, TVal>> + 'a
    where
        T: Clone,
        TVal: Clone
    {
        let distance = self.local_key.distance(target);
        ClosestIter {
            target,
            iter: None,
            table: self,
            buckets_iter: ClosestBucketsIter::new(distance),
            fmap: |b: &KBucket<_, TVal>| -> ArrayVec<_> {
                b.iter().map(|(n, status)| EntryView {
                    node: n.clone(),
                    status
                }).collect()
            }
        }
    }
}

/// An iterator over (some projection of) the closest entries in a
/// `KBucketsTable` w.r.t. some target `Key`.
struct ClosestIter<'a, TTarget, TPeerId, TVal, TMap, TOut> {
    /// A reference to the target key whose distance to the local key determines
    /// the order in which the buckets are traversed. The resulting
    /// array from projecting the entries of each bucket using `fmap` is
    /// sorted according to the distance to the target.
    target: &'a Key<TTarget>,
    /// A reference to all buckets of the `KBucketsTable`.
    table: &'a mut KBucketsTable<TPeerId, TVal>,
    /// The iterator over the bucket indices in the order determined by the
    /// distance of the local key to the target.
    buckets_iter: ClosestBucketsIter,
    /// The iterator over the entries in the currently traversed bucket.
    iter: Option<arrayvec::IntoIter<[TOut; K_VALUE]>>,
    /// The projection function / mapping applied on each bucket as
    /// it is encountered, producing the next `iter`ator.
    fmap: TMap
}

/// An iterator over the bucket indices, in the order determined by the `Distance` of
/// a target from the `local_key`, such that the entries in the buckets are incrementally
/// further away from the target, starting with the bucket covering the target.
struct ClosestBucketsIter {
    /// The distance to the `local_key`.
    distance: Distance,
    /// The current state of the iterator.
    state: ClosestBucketsIterState
}

/// Operating states of a `ClosestBucketsIter`.
enum ClosestBucketsIterState {
    /// The starting state of the iterator yields the first bucket index and
    /// then transitions to `ZoomIn`.
    Start(BucketIndex),
    /// The iterator "zooms in" to to yield the next bucket cotaining nodes that
    /// are incrementally closer to the local node but further from the `target`.
    /// These buckets are identified by a `1` in the corresponding bit position
    /// of the distance bit string. When bucket `0` is reached, the iterator
    /// transitions to `ZoomOut`.
    ZoomIn(BucketIndex),
    /// Once bucket `0` has been reached, the iterator starts "zooming out"
    /// to buckets containing nodes that are incrementally further away from
    /// both the local key and the target. These are identified by a `0` in
    /// the corresponding bit position of the distance bit string. When bucket
    /// `255` is reached, the iterator transitions to state `Done`.
    ZoomOut(BucketIndex),
    /// The iterator is in this state once it has visited all buckets.
    Done
}

impl ClosestBucketsIter {
    fn new(distance: Distance) -> Self {
        let state = match BucketIndex::new(&distance) {
            Some(i) => ClosestBucketsIterState::Start(i),
            None => ClosestBucketsIterState::Start(BucketIndex(0))
        };
        Self { distance, state }
    }

    fn next_in(&self, i: BucketIndex) -> Option<BucketIndex> {
        (0 .. i.get()).rev().find_map(|i|
            if self.distance.0.bit(i) {
                Some(BucketIndex(i))
            } else {
                None
            })
    }

    fn next_out(&self, i: BucketIndex) -> Option<BucketIndex> {
        (i.get() + 1 .. NUM_BUCKETS).find_map(|i|
            if !self.distance.0.bit(i) {
                Some(BucketIndex(i))
            } else {
                None
            })
    }
}

impl Iterator for ClosestBucketsIter {
    type Item = BucketIndex;

    fn next(&mut self) -> Option<Self::Item> {
        match self.state {
            ClosestBucketsIterState::Start(i) => {
                self.state = ClosestBucketsIterState::ZoomIn(i);
                Some(i)
            }
            ClosestBucketsIterState::ZoomIn(i) =>
                if let Some(i) = self.next_in(i) {
                    self.state = ClosestBucketsIterState::ZoomIn(i);
                    Some(i)
                } else {
                    let i = BucketIndex(0);
                    self.state = ClosestBucketsIterState::ZoomOut(i);
                    Some(i)
                }
            ClosestBucketsIterState::ZoomOut(i) =>
                if let Some(i) = self.next_out(i) {
                    self.state = ClosestBucketsIterState::ZoomOut(i);
                    Some(i)
                } else {
                    self.state = ClosestBucketsIterState::Done;
                    None
                }
            ClosestBucketsIterState::Done => None
        }
    }
}

impl<TTarget, TPeerId, TVal, TMap, TOut> Iterator
for ClosestIter<'_, TTarget, TPeerId, TVal, TMap, TOut>
where
    TPeerId: Clone,
    TMap: Fn(&KBucket<TPeerId, TVal>) -> ArrayVec<[TOut; K_VALUE]>,
    TOut: AsRef<Key<TPeerId>>
{
    type Item = TOut;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match &mut self.iter {
                Some(iter) => match iter.next() {
                    Some(k) => return Some(k),
                    None => self.iter = None
                }
                None => {
                    if let Some(i) = self.buckets_iter.next() {
                        let bucket = &mut self.table.buckets[i.get()];
                        if let Some(applied) = bucket.apply_pending() {
                            self.table.applied_pending.push_back(applied)
                        }
                        let mut v = (self.fmap)(bucket);
                        v.sort_by(|a, b|
                            self.target.distance(a.as_ref())
                                .cmp(&self.target.distance(b.as_ref())));
                        self.iter = Some(v.into_iter());
                    } else {
                        return None
                    }
                }
            }
        }
    }
}

/// A reference to a bucket in a `KBucketsTable`.
pub struct KBucketRef<'a, TPeerId, TVal> {
    index: BucketIndex,
    bucket: &'a mut KBucket<TPeerId, TVal>
}

impl<TPeerId, TVal> KBucketRef<'_, TPeerId, TVal>
where
    TPeerId: Clone
{
    /// Returns the number of entries in the bucket.
    pub fn num_entries(&self) -> usize {
        self.bucket.num_entries()
    }

    /// Returns true if the bucket has a pending node.
    pub fn has_pending(&self) -> bool {
        self.bucket.pending().map_or(false, |n| !n.is_ready())
    }

    pub fn contains(&self, d: &Distance) -> bool {
        BucketIndex::new(d).map_or(false, |i| i == self.index)
    }

    /// Generates a random distance that falls into this bucket.
    ///
    /// Together with a known key `a` (e.g. the local key), a random distance `d` for
    /// this bucket w.r.t `k` gives rise to the corresponding (random) key `b` s.t.
    /// the XOR distance between `a` and `b` is `d`. In other words, it gives
    /// rise to a random key falling into this bucket. See [`Key::from_distance`].
    pub fn rand_distance(&self, rng: &mut impl rand::Rng) -> Distance {
        self.index.rand_distance(rng)
    }
}

#[cfg(test)]
mod tests {
    use bigint::U256;
    use super::*;
    use libp2p_core::PeerId;
    use quickcheck::*;

    #[test]
    fn rand_distance() {
        fn prop(ix: u8) -> bool {
            let d = BucketIndex(ix as usize).rand_distance(&mut rand::thread_rng());
            let n = U256::from(<[u8; 32]>::from(d.0));
            let b = U256::from(2);
            let e = U256::from(ix);
            let lower = b.pow(e);
            let upper = b.pow(e + U256::from(1)) - U256::from(1);
            lower <= n && n <= upper
        }
        quickcheck(prop as fn(_) -> _);
    }

    #[test]
    fn basic_closest() {
        let local_key = Key::from(PeerId::random());
        let other_id = Key::from(PeerId::random());

        let mut table = KBucketsTable::<_, ()>::new(local_key, Duration::from_secs(5));
        if let Entry::Absent(entry) = table.entry(&other_id) {
            match entry.insert((), NodeStatus::Connected) {
                InsertResult::Inserted => (),
                _ => panic!()
            }
        } else {
            panic!()
        }

        let res = table.closest_keys(&other_id).collect::<Vec<_>>();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], other_id);
    }

    #[test]
    fn update_local_id_fails() {
        let local_key = Key::from(PeerId::random());
        let mut table = KBucketsTable::<_, ()>::new(local_key.clone(), Duration::from_secs(5));
        match table.entry(&local_key) {
            Entry::SelfEntry => (),
            _ => panic!(),
        }
    }

    #[test]
    fn closest() {
        let local_key = Key::from(PeerId::random());
        let mut table = KBucketsTable::<_, ()>::new(local_key, Duration::from_secs(5));
        let mut count = 0;
        loop {
            if count == 100 { break; }
            let key = Key::from(PeerId::random());
            if let Entry::Absent(e) = table.entry(&key) {
                match e.insert((), NodeStatus::Connected) {
                    InsertResult::Inserted => count += 1,
                    _ => continue,
                }
            } else {
                panic!("entry exists")
            }
        }

        let mut expected_keys: Vec<_> = table.buckets
            .iter()
            .flat_map(|t| t.iter().map(|(n,_)| n.key.clone()))
            .collect();

        for _ in 0 .. 10 {
            let target_key = Key::from(PeerId::random());
            let keys = table.closest_keys(&target_key).collect::<Vec<_>>();
            // The list of keys is expected to match the result of a full-table scan.
            expected_keys.sort_by_key(|k| k.distance(&target_key));
            assert_eq!(keys, expected_keys);
        }
    }

    #[test]
    fn applied_pending() {
        let local_key = Key::from(PeerId::random());
        let mut table = KBucketsTable::<_, ()>::new(local_key.clone(), Duration::from_millis(1));
        let expected_applied;
        let full_bucket_index;
        loop {
            let key = Key::from(PeerId::random());
            if let Entry::Absent(e) = table.entry(&key) {
                match e.insert((), NodeStatus::Disconnected) {
                    InsertResult::Full => {
                        if let Entry::Absent(e) = table.entry(&key) {
                            match e.insert((), NodeStatus::Connected) {
                                InsertResult::Pending { disconnected } => {
                                    expected_applied = AppliedPending {
                                        inserted: key.clone(),
                                        evicted: Some(Node { key: disconnected, value: () })
                                    };
                                    full_bucket_index = BucketIndex::new(&key.distance(&local_key));
                                    break
                                },
                                _ => panic!()
                            }
                        } else {
                            panic!()
                        }
                    },
                    _ => continue,
                }
            } else {
                panic!("entry exists")
            }
        }

        // Expire the timeout for the pending entry on the full bucket.`
        let full_bucket = &mut table.buckets[full_bucket_index.unwrap().get()];
        let elapsed = Instant::now() - Duration::from_secs(1);
        full_bucket.pending_mut().unwrap().set_ready_at(elapsed);

        match table.entry(&expected_applied.inserted) {
            Entry::Present(_, NodeStatus::Connected) => {}
            x => panic!("Unexpected entry: {:?}", x)
        }

        match table.entry(&expected_applied.evicted.as_ref().unwrap().key) {
            Entry::Absent(_) => {}
            x => panic!("Unexpected entry: {:?}", x)
        }

        assert_eq!(Some(expected_applied), table.take_applied_pending());
        assert_eq!(None, table.take_applied_pending());
    }
}
