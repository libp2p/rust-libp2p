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
// "LRU"-based ordering of entries in a bucket is actually based on the connection
// status of the corresponding peers, from least-recently connected to
// most-recently connected, and controlled through the `Entry` API. Entries in
// the buckets are not reordered as a result of RPC activity, but only as a
// result of entries being marked as connected or disconnected.
//
// [0]: https://pdos.csail.mit.edu/~petar/papers/maymounkov-kademlia-lncs.pdf

mod entry;

pub use entry::*;

use arrayvec::{self, ArrayVec};
use bigint::U256;
use libp2p_core::PeerId;
use multihash::Multihash;
use sha2::{Digest, Sha256, digest::generic_array::{GenericArray, typenum::U32}};
use std::slice;
use std::time::{Duration, Instant};

/// Maximum number of k-buckets.
const NUM_BUCKETS: usize = 256;

/// Maximum number of nodes in a bucket, i.e. the (currently fixed) `k` parameter.
pub const MAX_NODES_PER_BUCKET: usize = 20;

/// A `KBucketsTable` represents a Kademlia routing table.
#[derive(Debug, Clone)]
pub struct KBucketsTable<TPeerId, TVal> {
    /// The key identifying the local peer that owns the routing table.
    local_key: Key<TPeerId>,
    /// The buckets comprising the routing table.
    tables: Vec<KBucket<TPeerId, TVal>>,
    /// The timeout when trying to reach the least-recently connected node after
    /// which we consider it unresponsive.
    unresponsive_timeout: Duration,
}

/// A `Key` is a cryptographic hash, stored with an associated value in a `KBucket`
/// of a `KBucketsTable`.
///
/// `Key`s have an XOR metric as defined in the Kademlia paper, i.e. the bitwise XOR of
/// the hash digests, interpreted as an integer. See [`Key::distance`].
///
/// A `Key` preserves the preimage of type `T` of the hash function. See [`Key::preimage`].
#[derive(Clone, Debug)]
pub struct Key<T> {
    preimage: T,
    hash: GenericArray<u8, U32>,
}

impl<T> PartialEq for Key<T> {
    fn eq(&self, other: &Key<T>) -> bool {
        self.hash == other.hash
    }
}

impl<T> Eq for Key<T> {}

impl<T> Key<T> {
    /// Construct a new `Key` by hashing the bytes of the given `preimage`.
    ///
    /// The preimage of type `T` is preserved. See [`Key::preimage`] and
    /// [`Key::into_preimage`].
    pub fn new(preimage: T) -> Key<T>
    where
        T: AsRef<[u8]>
    {
        let hash = Sha256::digest(preimage.as_ref());
        Key { preimage, hash }
    }

    /// Borrows the preimage of the key.
    pub fn preimage(&self) -> &T {
        &self.preimage
    }

    /// Converts the key into its preimage.
    pub fn into_preimage(self) -> T {
        self.preimage
    }

    /// Computes the distance of the keys according to the XOR metric.
    pub fn distance<U>(&self, other: &Key<U>) -> Distance {
        let a = U256::from(self.hash.as_ref());
        let b = U256::from(other.hash.as_ref());
        Distance(a ^ b)
    }
}

impl From<Multihash> for Key<Multihash> {
    fn from(h: Multihash) -> Self {
        let k = Key::new(h.clone().into_bytes());
        Key { preimage: h, hash: k.hash }
    }
}

impl From<PeerId> for Key<PeerId> {
    fn from(peer_id: PeerId) -> Self {
        Key::new(peer_id)
    }
}

/// A distance between two `Key`s.
#[derive(Copy, Clone, PartialEq, Eq, Default, PartialOrd, Ord, Debug)]
pub struct Distance(bigint::U256);

/// A `KBucket` is a list of up to `MAX_NODES_PER_BUCKET` `Key`s and associated values,
/// ordered from least recently used to most recently used.
#[derive(Debug, Clone)]
pub struct KBucket<TPeerId, TVal> {
    /// Nodes are always ordered from least-recently connected to most-recently connected.
    nodes: ArrayVec<[Node<TPeerId, TVal>; MAX_NODES_PER_BUCKET]>,

    /// The position (index) in `nodes` that marks the first entry whose corresponding
    /// peer is considered connected.
    ///
    /// Since the entries in `nodes` are ordered from least-recently connected to
    /// most-recently connected, all entries above this index are also considered
    /// connected, i.e. the range `[0, first_connected_pos)` marks entries
    /// whose nodes are considered disconnected and the range
    /// `[first_connected_pos, MAX_NODES_PER_BUCKET)` marks entries whose
    /// nodes are considered connected.
    ///
    /// `None` indicates that there are no connected entries in the bucket, i.e.
    /// the bucket is either empty, or contains only entries for peers that are
    /// considered disconnected.
    first_connected_pos: Option<usize>,

    /// A node that is pending to be inserted into a full bucket, should the
    /// least-recently connected (and currently disconnected) node not reconnect.
    pending_node: Option<PendingNode<TPeerId, TVal>>,
}

impl<TPeerId, TVal> KBucket<TPeerId, TVal> {
    /// TODO
    fn apply_pending(&mut self) {
        if let Some(pending) = self.pending_node.take() {
            if pending.replace <= Instant::now() {
                // If there is a pending entry, then there must be at least one
                // entry marked as disconnected and the bucket must be full.
                debug_assert!(self.first_connected_pos.map_or(true, |p| p > 0));
                debug_assert!(self.nodes.len() == MAX_NODES_PER_BUCKET);
                // Remove the entry of the least-recently connected peer.
                self.nodes.remove(0);
                if pending.connected {
                    self.first_connected_pos = self.first_connected_pos.or(Some(self.nodes.len()));
                    self.nodes.push(pending.node);
                }
                // A disconnected pending node goes at the end of the entries
                // for the disconnected peers.
                else if let Some(p) = self.first_connected_pos {
                    if p > 0 {
                        self.nodes.insert(p - 1, pending.node);
                    }
                } else {
                    self.nodes.push(pending.node);
                }
            } else {
                self.pending_node = Some(pending)
            }
        }
    }
}

/// State of the pending node.
#[derive(Debug, Clone)]
struct PendingNode<TPeerId, TVal> {
    /// Node to insert.
    node: Node<TPeerId, TVal>,

    /// If true, we are connected to the pending node.
    connected: bool,

    /// When the pending node will replace an existing node, provided that the oldest node
    /// doesn't become responsive before.
    replace: Instant,
}

/// A single node in a `KBucket`.
#[derive(Debug, Clone)]
struct Node<TPeerId, TVal> {
    /// Id of the node.
    id: Key<TPeerId>,
    /// Value associated to it.
    value: TVal,
}

/// A (safe) index into a `KBucketsTable`, i.e. a non-negative integer in the
/// range `[0, NUM_BUCKETS)`.
#[derive(Copy, Clone)]
struct BucketIndex(usize);

impl BucketIndex {
    /// Creates a new `BucketIndex` for a `Distance` from the `local_key`.
    ///
    /// If the distance is zero, `None` is returned, in recognition of the fact that
    /// the only key with distance `0` to the `local_key` is the `local_key` itself,
    /// which does not belong in any bucket.
    fn of(d: &Distance) -> Option<BucketIndex> { // TODO: new
        (NUM_BUCKETS - d.0.leading_zeros() as usize)
            .checked_sub(1)
            .map(BucketIndex)
    }

    /// Gets the index value as an unsigned integer.
    fn get(&self) -> usize {
        self.0
    }
}

impl<TPeerId> AsRef<Key<TPeerId>> for Key<TPeerId> { // Borrow?
    fn as_ref(&self) -> &Key<TPeerId> {
        self
    }
}

impl<TPeerId, TVal> KBucketsTable<TPeerId, TVal>
where
    TPeerId: Clone,
{
    /// Builds a new routing table whose keys are distributed over `KBucket`s as
    /// per the relative distance to `local_key`.
    pub fn new(local_key: Key<TPeerId>, unresponsive_timeout: Duration) -> Self {
        KBucketsTable {
            local_key,
            tables: (0 .. NUM_BUCKETS).map(|_| KBucket {
                nodes: ArrayVec::new(),
                first_connected_pos: None,
                pending_node: None,
            })
            .collect(),
            unresponsive_timeout,
        }
    }

    /// Returns the local key.
    pub fn local_key(&self) -> &Key<TPeerId> {
        &self.local_key
    }

    fn bucket(&self, key: &Key<TPeerId>) -> Option<&KBucket<TPeerId, TVal>> {
        BucketIndex::of(&self.local_key.distance(key)).map(|i| &self.tables[i.get()])
    }

    fn bucket_mut(&mut self, key: &Key<TPeerId>) -> Option<&mut KBucket<TPeerId, TVal>> {
        BucketIndex::of(&self.local_key.distance(key)).map(move |i| &mut self.tables[i.get()])
    }

    /// Returns an object containing the state of the given entry.
    pub fn entry<'a>(&'a mut self, peer_id: &'a Key<TPeerId>) -> Entry<'a, TPeerId, TVal> {
        Entry::new(self, peer_id)
    }

    /// Returns an iterator over all the entries in the bucket, excluding those that
    /// are pending.
    pub fn entries_not_pending(&self) -> impl Iterator<Item = (&Key<TPeerId>, &TVal)> {
        self.tables
            .iter()
            .flat_map(|table| table.nodes.iter())
            .map(|node| (&node.id, &node.value))
    }

    /// Returns an iterator over all buckets.
    ///
    /// The buckets are ordered by proximity to the `local_key`, i.e. the first
    /// bucket is the closest bucket (containing at most one key).
    pub fn buckets(&mut self) -> KBucketsIter<'_, TPeerId, TVal> {
        KBucketsIter(self.tables.iter_mut(), self.unresponsive_timeout)
    }

    /// Creates an iterator over the keys closest to `target`, ordered by
    /// increasing distance.
    pub fn closest_keys<'a, T>(&'a mut self, target: &'a Key<T>)
        -> impl Iterator<Item = Key<TPeerId>> + 'a
    where
        T: Clone
    {
        let distance = self.local_key.distance(target);
        let buckets_iter = ClosestBucketsIter::new(distance);
        ClosestIter {
            target,
            iter: None,
            buckets: &mut self.tables,
            buckets_iter,
            fmap: |b: &KBucket<_, _>| -> ArrayVec<_> {
                b.nodes.iter().map(|n| n.id.clone()).collect()
            }
        }
    }

    /// Creates an iterator over the entries closest to `target`, ordered by
    /// increasing distance.
    pub fn closest<'a, T>(&'a mut self, target: &'a Key<T>)
        -> impl Iterator<Item = EntryView<TPeerId, TVal>> + 'a
    where
        T: Clone,
        TVal: Clone
    {
        let distance = self.local_key.distance(target);
        let buckets_iter = ClosestBucketsIter::new(distance);
        ClosestIter {
            target,
            iter: None,
            buckets: &mut self.tables,
            buckets_iter,
            fmap: |b: &KBucket<_, TVal>| -> ArrayVec<_> {
                b.nodes.iter().enumerate().map(|(i, n)| EntryView {
                    key: n.id.clone(),
                    value: n.value.clone(),
                    connected: b.first_connected_pos.map_or(false, |j| i >= j)
                }).collect()
            }
        }
    }

}

/// TODO
pub struct ClosestIter<'a, TTarget, TPeerId, TVal, TMap, TOut> {
    target: &'a Key<TTarget>,
    buckets: &'a mut Vec<KBucket<TPeerId, TVal>>,
    buckets_iter: ClosestBucketsIter,
    iter: Option<arrayvec::IntoIter<[TOut; MAX_NODES_PER_BUCKET]>>,
    fmap: TMap
}

/// An iterator over all keys in a bucket, sorted by distance to a target key.
type ClosestKeysIter<T> = arrayvec::IntoIter<[Key<T>; MAX_NODES_PER_BUCKET]>;

/// An iterator over the bucket indices, in the order determined the `Distance` of
/// a target from the `local_key`, such that the nodes in the buckets are incrementally
/// further away from the target, starting with the bucket covering the target.
struct ClosestBucketsIter {
    /// The distance to the `local_key`.
    distance: Distance,
    /// The current state of the iterator.
    state: ClosestBucketsIterState
}

/// Operating states of a `ClosestBucketsIter`.
enum ClosestBucketsIterState {
    Start(BucketIndex),
    /// Beginning with the bucket into which the `target` key falls, the
    /// iterator "zooms in" to buckets cotaining nodes that are incrementally
    /// closer to the local node but further from the `target`. These are
    /// identified by a `1` in the corresponding bit position of the distance
    /// bit string. When bucket `0` is reached, the iterator transitions to
    /// state `ZoomOut`.
    ZoomIn(BucketIndex),
    /// Once bucket `0` has been reached, the iterator starts "zooming out"
    /// to buckets containing nodes that are incrementally further away from
    /// both the local node and the target. These are identified by a `0` in
    /// the corresponding bit position of the distance bit string. When bucket
    /// `255` is reached, the iterator transitions to state `Done`.
    ZoomOut(BucketIndex),
    /// The iterator is in this state once it has visited all buckets.
    Done
}

impl ClosestBucketsIter {
    fn new(distance: Distance) -> Self {
        let state = match BucketIndex::of(&distance) {
            Some(i) => ClosestBucketsIterState::Start(i),
            None => ClosestBucketsIterState::Done
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
    TMap: Fn(&KBucket<TPeerId, TVal>) -> ArrayVec<[TOut; MAX_NODES_PER_BUCKET]>,
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
                        let bucket = &mut self.buckets[i.get()];
                        bucket.apply_pending();
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

/// By-reference iterator over `KBucket`s in the order of their increasing distance
/// to the local key.
pub struct KBucketsIter<'a, TPeerId, TVal>(slice::IterMut<'a, KBucket<TPeerId, TVal>>, Duration);

impl<'a, TPeerId, TVal> Iterator for KBucketsIter<'a, TPeerId, TVal> {
    type Item = KBucketRef<'a, TPeerId, TVal>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(KBucketRef)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<'a, TPeerId, TVal> ExactSizeIterator for KBucketsIter<'a, TPeerId, TVal> {}

/// A reference to a `KBucket`.
pub struct KBucketRef<'a, TPeerId, TVal>(&'a mut KBucket<TPeerId, TVal>);

impl<'a, TPeerId, TVal> KBucketRef<'a, TPeerId, TVal> {
    /// Returns the number of entries in that bucket.
    ///
    /// > **Note**: Keep in mind that this operation can be racy. If `update()` is called on the
    /// >           table while this function is running, the `update()` may or may not be taken
    /// >           into account.
    pub fn num_entries(&self) -> usize {
        self.0.nodes.len()
    }

    /// Returns true if this bucket has a pending node.
    pub fn has_pending(&self) -> bool {
        if let Some(ref node) = self.0.pending_node {
            node.replace > Instant::now()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;
    use libp2p_core::PeerId;
    use crate::kbucket::{Entry, InsertOutcome, KBucketsTable, MAX_NODES_PER_BUCKET};
    use std::time::Duration;

    impl Arbitrary for Key<PeerId> {
        fn arbitrary<G: Gen>(_: &mut G) -> Key<PeerId> {
            Key::from(PeerId::random())
        }
    }

    #[test]
    fn identity() {
        fn prop(a: Key<PeerId>) -> bool {
            a.distance(&a) == Distance::default()
        }
        quickcheck(prop as fn(_) -> _)
    }

    #[test]
    fn symmetry() {
        fn prop(a: Key<PeerId>, b: Key<PeerId>) -> bool {
            a.distance(&b) == b.distance(&a)
        }
        quickcheck(prop as fn(_,_) -> _)
    }

    #[test]
    fn triangle_inequality() {
        fn prop(a: Key<PeerId>, b: Key<PeerId>, c: Key<PeerId>) -> TestResult {
            let ab = a.distance(&b);
            let bc = b.distance(&c);
            let (ab_plus_bc, overflow) = ab.0.overflowing_add(bc.0);
            if overflow {
                TestResult::discard()
            } else {
                TestResult::from_bool(a.distance(&c) <= Distance(ab_plus_bc))
            }
        }
        quickcheck(prop as fn(_,_,_) -> _)
    }

    #[test]
    fn unidirectionality() {
        fn prop(a: Key<PeerId>, b: Key<PeerId>) -> bool {
            let d = a.distance(&b);
            (0..100).all(|_| {
                let c = Key::from(PeerId::random());
                a.distance(&c) != d || b == c
            })
        }
        quickcheck(prop as fn(_,_) -> _)
    }


    #[test]
    fn basic_closest() {
        let my_key = Key::from(PeerId::random());
        let other_id = Key::from(PeerId::random());

        let mut table = KBucketsTable::<_, ()>::new(my_key, Duration::from_secs(5));
        if let Entry::NotInKbucket(entry) = table.entry(&other_id) {
            match entry.insert_connected(()) {
                InsertOutcome::Inserted => (),
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
        let my_key = Key::from(PeerId::random());

        let mut table = KBucketsTable::<_, ()>::new(my_key.clone(), Duration::from_secs(5));
        match table.entry(&my_key) {
            Entry::SelfEntry => (),
            _ => panic!(),
        }
    }

    #[test]
    fn full_kbucket() {
        let my_key = Key::from(PeerId::random());

        let mut table = KBucketsTable::<_, ()>::new(my_key.clone(), Duration::from_secs(5));

        // Step 1: Fill the most distant bucket, i.e. bucket index `NUM_BUCKETS - 1`,
        // with "disconnected" peers.

        // Prepare `MAX_NODES_PER_BUCKET` keys to fill the bucket, plus 2
        // additional keys which will be used to test the behavior on a full
        // bucket.
        assert!(MAX_NODES_PER_BUCKET <= 251); // Test doesn't work otherwise.
        let mut fill_ids = (0..MAX_NODES_PER_BUCKET + 3)
            .map(|n| {
                let mut id = my_key.clone();
                // Flip the first bit so that we get in the most distant bucket.
                id.hash[0] ^= 0x80;
                // Each ID gets a unique low-order byte (i.e. counter).
                id.hash[31] = id.hash[31].wrapping_add(n as u8);
                id
            })
            .collect::<Vec<_>>();

        fn last_bucket(table: &mut KBucketsTable<PeerId, ()>) -> &mut KBucket<PeerId, ()> {
            &mut table.tables[255]
        }

        let first_node = fill_ids[0].clone();
        let second_node = fill_ids[1].clone();

        // Fill the bucket, consuming all but the last 3 test keys.
        for (num, id) in fill_ids.drain(..MAX_NODES_PER_BUCKET).enumerate() {
            if let Entry::NotInKbucket(entry) = table.entry(&id) {
                match entry.insert_disconnected(()) {
                    InsertOutcome::Inserted => (),
                    _ => panic!()
                }
            } else {
                panic!()
            }
            assert_eq!(last_bucket(&mut table).nodes.len(), num + 1);
        }
        assert_eq!(last_bucket(&mut table).nodes.len(), MAX_NODES_PER_BUCKET);
        assert!(last_bucket(&mut table).pending_node.is_none());

        // Step 2: Insert another key on the full bucket. It must be marked as
        // pending and the first (i.e. "least recently used") entry scheduled
        // for replacement.
        let replacement = fill_ids.remove(0);
        if let Entry::NotInKbucket(entry) = table.entry(&replacement) {
            match entry.insert_connected(()) {
                InsertOutcome::Pending { ref to_ping } if *to_ping == first_node => (),
                _ => panic!()
            }
        } else {
            panic!()
        }
        let pending = last_bucket(&mut table).pending_node.as_ref().map(|n| &n.node.id);
        assert_eq!(pending, Some(&replacement));
        assert_eq!(last_bucket(&mut table).nodes.len(), MAX_NODES_PER_BUCKET);
        // Trying to insert yet another key is rejected.
        if let Entry::NotInKbucket(entry) = table.entry(&Key::from(fill_ids.remove(0))) {
            match entry.insert_connected(()) {
                InsertOutcome::Full => (),
                _ => panic!()
            }
        } else {
            panic!()
        }

        // Step 3: Make the pending nodes eligible for replacing existing nodes.
        // The pending node must be consumed and replace the first (i.e. "least
        // recently connected") node.
        let elapsed = Instant::now() - Duration::from_secs(1);
        last_bucket(&mut table).pending_node.as_mut().map(|n| n.replace = elapsed);
        let replacement2 = fill_ids.remove(0);
        if let Entry::NotInKbucket(entry) = table.entry(&replacement2) {
            match entry.insert_connected(()) {
                InsertOutcome::Pending { ref to_ping } if *to_ping == second_node => (),
                e => panic!("{:?}", e)
            }
        } else {
            panic!()
        }

        // The replacement must now be in the bucket and considered connected.
        match table.entry(&replacement) {
            Entry::InKbucketConnected(_) => {},
            _ => panic!()
        }
        let pending = last_bucket(&mut table).pending_node.as_ref().map(|n| &n.node.id);
        assert_eq!(pending, Some(&replacement2));
    }

    #[test]
    fn closest() {
        let local_key = Key::from(PeerId::random());
        let mut table = KBucketsTable::<_, ()>::new(local_key, Duration::from_secs(5));
        let mut count = 0;
        loop {
            if count == 100 { break; }
            let key = Key::from(PeerId::random());
            if let Entry::NotInKbucket(e) = table.entry(&key) {
                match e.insert_connected(()) {
                    InsertOutcome::Inserted => count += 1,
                    _ => continue,
                }
            } else {
                panic!("entry exists")
            }
        }

        let mut expected_keys: Vec<_> = table.tables
            .iter()
            .flat_map(|t| t.nodes.iter().map(|n| n.id.clone()))
            .collect();

        for _ in 0 .. 10 {
            let target_key = Key::from(PeerId::random());
            let keys = table.closest_keys(&target_key).collect::<Vec<_>>();
            // The list of keys is expected to match the result of a full-table scan.
            expected_keys.sort_by_key(|k| k.distance(&target_key));
            assert_eq!(keys, expected_keys);
        }
    }
}
