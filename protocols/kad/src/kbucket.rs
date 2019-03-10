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

//! Key-value storage, with a refresh and a time-to-live system.
//!
//! A k-buckets table allows one to store a value identified by keys, ordered by their distance
//! to a reference key passed to the constructor.
//!
//! If the local ID has `N` bits, then the k-buckets table contains `N` *buckets* each containing
//! a constant number of entries. Storing a key in the k-buckets table adds it to the bucket
//! corresponding to its distance with the reference key.

use arrayvec::ArrayVec;
use bigint::U512;
use libp2p_core::PeerId;
use multihash::Multihash;
use std::slice::IterMut as SliceIterMut;
use std::time::{Duration, Instant};
use std::vec::IntoIter as VecIntoIter;

/// Maximum number of nodes in a bucket.
pub const MAX_NODES_PER_BUCKET: usize = 20;

/// Table of k-buckets.
#[derive(Debug, Clone)]
pub struct KBucketsTable<TPeerId, TVal> {
    /// Peer ID of the local node.
    my_id: TPeerId,
    /// The actual tables that store peers or values.
    tables: Vec<KBucket<TPeerId, TVal>>,
    /// The timeout when trying to reach the first node after which we consider it unresponsive.
    unresponsive_timeout: Duration,
}

/// An individual table that stores peers or values.
#[derive(Debug, Clone)]
struct KBucket<TPeerId, TVal> {
    /// Nodes are always ordered from oldest to newest. The nodes we are connected to are always
    /// all on top of the nodes we are not connected to.
    nodes: ArrayVec<[Node<TPeerId, TVal>; MAX_NODES_PER_BUCKET]>,

    /// Index in `nodes` over which all nodes are connected. Must always be <= to the length
    /// of `nodes`.
    first_connected_pos: usize,

    /// Node received when the bucket was full. Will be added to the list if the first node doesn't
    /// respond in time to our reach attempt. The second element is the time when the pending node
    /// was added. If it is too old we drop the first node and add the pending node to the end of
    /// the list.
    pending_node: Option<(Node<TPeerId, TVal>, Instant)>,

    /// Last time this bucket was updated.
    latest_update: Instant,
}

/// A single node in a k-bucket.
#[derive(Debug, Clone)]
struct Node<TPeerId, TVal> {
    /// Id of the node.
    id: TPeerId,
    /// Value associated to it.
    value: TVal,
}

impl<TPeerId, TVal> KBucket<TPeerId, TVal> {
    /// Puts the kbucket into a coherent state.
    /// If a node is pending and the timeout has expired, removes the first element of `nodes`
     /// and puts the node back in `pending_node`.
    fn flush(&mut self, timeout: Duration) {
        if let Some((pending_node, instant)) = self.pending_node.take() {
            if instant.elapsed() >= timeout {
                let _ = self.nodes.remove(0);
                self.nodes.push(pending_node);
            } else {
                self.pending_node = Some((pending_node, instant));
            }
        }
    }
}

/// Trait that must be implemented on types that can be used as an identifier in a k-bucket.
pub trait KBucketsPeerId<TOther = Self>: PartialEq<TOther> + Clone {
    /// Computes the XOR of this value and another one. The lower the closer.
    fn distance_with(&self, other: &TOther) -> u32;

    /// Returns then number of bits that are necessary to store the distance between peer IDs.
    /// Used for pre-allocations.
    ///
    /// > **Note**: Returning 0 would lead to a panic.
    fn max_distance() -> usize;
}

impl KBucketsPeerId for PeerId {
    #[inline]
    fn distance_with(&self, other: &Self) -> u32 {
        Multihash::distance_with(self.as_ref(), other.as_ref())
    }

    #[inline]
    fn max_distance() -> usize {
        <Multihash as KBucketsPeerId>::max_distance()
    }
}

impl KBucketsPeerId<Multihash> for PeerId {
    #[inline]
    fn distance_with(&self, other: &Multihash) -> u32 {
        Multihash::distance_with(self.as_ref(), other)
    }

    #[inline]
    fn max_distance() -> usize {
        <Multihash as KBucketsPeerId>::max_distance()
    }
}

impl KBucketsPeerId for Multihash {
    #[inline]
    fn distance_with(&self, other: &Self) -> u32 {
        // Note that we don't compare the hash functions because there's no chance of collision
        // of the same value hashed with two different hash functions.
        let my_hash = U512::from(self.digest());
        let other_hash = U512::from(other.digest());
        let xor = my_hash ^ other_hash;
        512 - xor.leading_zeros()
    }

    #[inline]
    fn max_distance() -> usize {
        512
    }
}

impl<TPeerId, TVal> KBucketsTable<TPeerId, TVal>
where
    TPeerId: KBucketsPeerId,
{
    /// Builds a new routing table.
    pub fn new(my_id: TPeerId, unresponsive_timeout: Duration) -> Self {
        KBucketsTable {
            my_id,
            tables: (0..TPeerId::max_distance())
                .map(|_| KBucket {
                    nodes: ArrayVec::new(),
                    first_connected_pos: 0,
                    pending_node: None,
                    latest_update: Instant::now(),
                })
                .collect(),
            unresponsive_timeout,
        }
    }

    // Returns the id of the bucket that should contain the peer with the given ID.
    //
    // Returns `None` if out of range, which happens if `id` is the same as the local peer id.
    #[inline]
    fn bucket_num(&self, id: &TPeerId) -> Option<usize> {
        (self.my_id.distance_with(id) as usize).checked_sub(1)
    }

    /// Returns an iterator to all the buckets of this table.
    ///
    /// Ordered by proximity to the local node. Closest bucket (with max. one node in it) comes
    /// first.
    #[inline]
    pub fn buckets(&mut self) -> BucketsIter<'_, TPeerId, TVal> {
        BucketsIter(self.tables.iter_mut(), self.unresponsive_timeout)
    }

    /// Returns the ID of the local node.
    #[inline]
    pub fn my_id(&self) -> &TPeerId {
        &self.my_id
    }

    /// Returns the value associated to a node, if any is present.
    ///
    /// Does **not** include pending nodes.
    pub fn get(&self, id: &TPeerId) -> Option<&TVal> {
        let table = match self.bucket_num(&id) {
            Some(n) => &self.tables[n],
            None => return None,
        };

        for elem in &table.nodes {
            if elem.id == *id {
                return Some(&elem.value);
            }
        }

        None
    }

    /// Returns the value associated to a node, if any is present.
    ///
    /// Does **not** include pending nodes.
    pub fn get_mut(&mut self, id: &TPeerId) -> Option<&mut TVal> {
        let table = match self.bucket_num(&id) {
            Some(n) => &mut self.tables[n],
            None => return None,
        };

        table.flush(self.unresponsive_timeout);

        for elem in &mut table.nodes {
            if elem.id == *id {
                return Some(&mut elem.value);
            }
        }

        None
    }

    /// Returns the value associated to a node if any is present. Otherwise, tries to add the
    /// node to the table in a disconnected state and return its value. Returns `None` if `id` is
    /// the local peer, or if the table is full.
    pub fn entry_mut(&mut self, id: &TPeerId) -> Option<&mut TVal>
    where
        TVal: Default,
    {
        if let Some((bucket, entry)) = self.entry_mut_inner(id) {
            Some(&mut self.tables[bucket].nodes[entry].value)
        } else {
            None
        }
    }

    /// Apparently non-lexical lifetimes still aren't working properly in some situations, so we
    /// delegate `entry_mut` to this method that returns an index within `self.tables` and the
    /// node index within that table.
    fn entry_mut_inner(&mut self, id: &TPeerId) -> Option<(usize, usize)>
    where
        TVal: Default,
    {
        let (bucket_num, table) = match self.bucket_num(&id) {
            Some(n) => (n, &mut self.tables[n]),
            None => return None,
        };

        table.flush(self.unresponsive_timeout);

        if let Some(pos) = table.nodes.iter().position(|elem| elem.id == *id) {
            return Some((bucket_num, pos));
        }

        if !table.nodes.is_full() {
            table.nodes.insert(table.first_connected_pos, Node {
                id: id.clone(),
                value: Default::default(),
            });
            table.first_connected_pos += 1;
            table.latest_update = Instant::now();
            return Some((bucket_num, table.first_connected_pos - 1));
        }

        None
    }

    /// Reports that we are connected to the given node.
    ///
    /// This inserts the node in the k-buckets, if possible. If it is already in a k-bucket, puts
    /// it above the disconnected nodes. If it is not already in a k-bucket, then the value will
    /// be built with the `Default` trait.
    pub fn set_connected(&mut self, id: &TPeerId) -> Update<'_, TPeerId>
    where
        TVal: Default,
    {
        let table = match self.bucket_num(&id) {
            Some(n) => &mut self.tables[n],
            None => return Update::FailSelfUpdate,
        };

        table.flush(self.unresponsive_timeout);

        if let Some(pos) = table.nodes.iter().position(|elem| elem.id == *id) {
            // Node is already in the table; move it over `first_connected_pos` if necessary.
            // We do a `saturating_sub(1)`, because if `first_connected_pos` is 0 then
            // `pos < first_connected_pos` can never be true anyway.
            if pos < table.first_connected_pos.saturating_sub(1) {
                let elem = table.nodes.remove(pos);
                table.first_connected_pos -= 1;
                table.nodes.insert(table.first_connected_pos, elem);
            }
            table.latest_update = Instant::now();
            Update::Updated

        } else if !table.nodes.is_full() {
            // Node is not in the table yet, but there's plenty of space for it.
            table.nodes.insert(table.first_connected_pos, Node {
                id: id.clone(),
                value: Default::default(),
            });
            table.latest_update = Instant::now();
            Update::Added

        } else if table.first_connected_pos > 0 && table.pending_node.is_none() {
            // Node is not in the table yet, but there could be room for it if we drop the first
            // element. However we first add the node to add to `pending_node` and try to reconnect
            // to the oldest node.
            let pending_node = Node {
                id: id.clone(),
                value: Default::default(),
            };
            table.pending_node = Some((pending_node, Instant::now()));
            Update::Pending(&table.nodes[0].id)

        } else {
            debug_assert!(table.first_connected_pos == 0 || table.pending_node.is_some());
            Update::Discarded
        }
    }

    /// Reports that we are now disconnected from the given node.
    ///
    /// This does *not* remove the node from the k-buckets, but moves it underneath the nodes we
    /// are still connected to.
    pub fn set_disconnected(&mut self, id: &TPeerId) {
        let table = match self.bucket_num(&id) {
            Some(n) => &mut self.tables[n],
            None => return,
        };

        table.flush(self.unresponsive_timeout);

        let pos = match table.nodes.iter().position(|elem| elem.id == *id) {
            Some(pos) => pos,
            None => return,
        };

        if pos > table.first_connected_pos {
            let elem = table.nodes.remove(pos);
            table.nodes.insert(table.first_connected_pos, elem);
            table.first_connected_pos += 1;
        } else if pos == table.first_connected_pos {
            table.first_connected_pos += 1;
        }
    }

    /// Finds the `num` nodes closest to `id`, ordered by distance.
    pub fn find_closest<TOther>(&mut self, id: &TOther) -> VecIntoIter<TPeerId>
    where
        TPeerId: Clone + KBucketsPeerId<TOther>,
    {
        // TODO: optimize
        let mut out = Vec::new();
        for table in self.tables.iter_mut() {
            table.flush(self.unresponsive_timeout);
            if table.latest_update.elapsed() > self.unresponsive_timeout {
                continue; // ignore bucket with expired nodes
            }
            for node in table.nodes.iter() {
                out.push(node.id.clone());
            }
        }
        out.sort_by(|a, b| b.distance_with(id).cmp(&a.distance_with(id)));
        out.into_iter()
    }

    /// Same as `find_closest`, but includes the local peer as well.
    pub fn find_closest_with_self<TOther>(&mut self, id: &TOther) -> VecIntoIter<TPeerId>
    where
        TPeerId: Clone + KBucketsPeerId<TOther>,
    {
        // TODO: optimize
        let mut intermediate: Vec<_> = self.find_closest(id).collect();
        if let Some(pos) = intermediate
            .iter()
            .position(|e| e.distance_with(id) >= self.my_id.distance_with(id))
        {
            if intermediate[pos] != self.my_id {
                intermediate.insert(pos, self.my_id.clone());
            }
        } else {
            intermediate.push(self.my_id.clone());
        }
        intermediate.into_iter()
    }
}

/// Return value of the `set_connected()` method.
#[derive(Debug)]
#[must_use]
pub enum Update<'a, TPeerId> {
    /// The node has been added to the bucket.
    Added,
    /// The node was already in the bucket and has been updated.
    Updated,
    /// The node has been added as pending. We need to try connect to the node passed as parameter.
    Pending(&'a TPeerId),
    /// The node wasn't added at all because a node was already pending.
    Discarded,
    /// Tried to update the local peer ID. This is an invalid operation.
    FailSelfUpdate,
}

/// Iterator giving access to a bucket.
pub struct BucketsIter<'a, TPeerId, TVal>(SliceIterMut<'a, KBucket<TPeerId, TVal>>, Duration);

impl<'a, TPeerId, TVal> Iterator for BucketsIter<'a, TPeerId, TVal> {
    type Item = Bucket<'a, TPeerId, TVal>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|bucket| {
            bucket.flush(self.1);
            Bucket(bucket)
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<'a, TPeerId, TVal> ExactSizeIterator for BucketsIter<'a, TPeerId, TVal> {}

/// Access to a bucket.
pub struct Bucket<'a, TPeerId, TVal>(&'a mut KBucket<TPeerId, TVal>);

impl<'a, TPeerId, TVal> Bucket<'a, TPeerId, TVal> {
    /// Returns the number of entries in that bucket.
    ///
    /// > **Note**: Keep in mind that this operation can be racy. If `update()` is called on the
    /// >           table while this function is running, the `update()` may or may not be taken
    /// >           into account.
    #[inline]
    pub fn num_entries(&self) -> usize {
        self.0.nodes.len()
    }

    /// Returns true if this bucket has a pending node.
    #[inline]
    pub fn has_pending(&self) -> bool {
        self.0.pending_node.is_some()
    }

    /// Returns the time when any of the values in this bucket was last updated.
    ///
    /// If the bucket is empty, this returns the time when the whole table was created.
    #[inline]
    pub fn latest_update(&self) -> Instant {
        self.0.latest_update
    }
}

#[cfg(test)]
mod tests {
    use rand::random;
    use crate::kbucket::{KBucketsPeerId, KBucketsTable, Update, MAX_NODES_PER_BUCKET};
    use multihash::{Multihash, Hash};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn basic_closest() {
        let my_id = Multihash::random(Hash::SHA2256);
        let other_id = Multihash::random(Hash::SHA2256);

        let mut table = KBucketsTable::<_, ()>::new(my_id, Duration::from_secs(5));
        table.entry_mut(&other_id);

        let res = table.find_closest(&other_id).collect::<Vec<_>>();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], other_id);
    }

    #[test]
    fn update_local_id_fails() {
        let my_id = Multihash::random(Hash::SHA2256);

        let mut table = KBucketsTable::<_, ()>::new(my_id.clone(), Duration::from_secs(5));
        assert!(table.entry_mut(&my_id).is_none());
        match table.set_connected(&my_id) {
            Update::FailSelfUpdate => (),
            _ => panic!(),
        }
    }

    #[test]
    fn update_time_last_refresh() {
        let my_id = Multihash::random(Hash::SHA2256);

        // Generate some other IDs varying by just one bit.
        let other_ids = (0..random::<usize>() % 20)
            .map(|_| {
                let bit_num = random::<usize>() % 256;
                let mut id = my_id.as_bytes().to_vec().clone();
                id[33 - (bit_num / 8)] ^= 1 << (bit_num % 8);
                (Multihash::from_bytes(id).unwrap(), bit_num)
            })
            .collect::<Vec<_>>();

        let mut table = KBucketsTable::<_, ()>::new(my_id, Duration::from_secs(5));
        let before_update = table.buckets().map(|b| b.latest_update()).collect::<Vec<_>>();

        thread::sleep(Duration::from_secs(2));
        for &(ref id, _) in &other_ids {
            table.entry_mut(&id);
        }

        let after_update = table.buckets().map(|b| b.latest_update()).collect::<Vec<_>>();

        for (offset, (bef, aft)) in before_update.iter().zip(after_update.iter()).enumerate() {
            if other_ids.iter().any(|&(_, bucket)| bucket == offset) {
                assert_ne!(bef, aft);
            } else {
                assert_eq!(bef, aft);
            }
        }
    }

    #[test]
    fn full_kbucket() {
        let my_id = Multihash::random(Hash::SHA2256);

        assert!(MAX_NODES_PER_BUCKET <= 251); // Test doesn't work otherwise.
        let mut fill_ids = (0..MAX_NODES_PER_BUCKET + 3)
            .map(|n| {
                let mut id = my_id.clone().into_bytes();
                id[2] ^= 0x80; // Flip the first bit so that we get in the most distant bucket.
                id[33] = id[33].wrapping_add(n as u8);
                Multihash::from_bytes(id).unwrap()
            })
            .collect::<Vec<_>>();

        let first_node = fill_ids[0].clone();
        let second_node = fill_ids[1].clone();

        let mut table = KBucketsTable::<_, ()>::new(my_id.clone(), Duration::from_secs(1));

        for (num, id) in fill_ids.drain(..MAX_NODES_PER_BUCKET).enumerate() {
            match table.set_connected(&id) {
                Update::Added => (),
                _ => panic!()
            }
            table.set_disconnected(&id);
            assert_eq!(table.buckets().nth(255).unwrap().num_entries(), num + 1);
        }

        assert_eq!(
            table.buckets().nth(255).unwrap().num_entries(),
            MAX_NODES_PER_BUCKET
        );
        assert!(!table.buckets().nth(255).unwrap().has_pending());
        match table.set_connected(&fill_ids.remove(0)) {
            Update::Pending(to_ping) => {
                assert_eq!(*to_ping, first_node);
            },
            _ => panic!()
        }

        assert_eq!(
            table.buckets().nth(255).unwrap().num_entries(),
            MAX_NODES_PER_BUCKET
        );
        assert!(table.buckets().nth(255).unwrap().has_pending());
        match table.set_connected(&fill_ids.remove(0)) {
            Update::Discarded => (),
            _ => panic!()
        }

        thread::sleep(Duration::from_secs(2));
        assert!(!table.buckets().nth(255).unwrap().has_pending());
        match table.set_connected(&fill_ids.remove(0)) {
            Update::Pending(to_ping) => {
                assert_eq!(*to_ping, second_node);
            },
            _ => panic!()
        }
    }

    #[test]
    fn self_distance_zero() {
        let a = Multihash::random(Hash::SHA2256);
        assert_eq!(a.distance_with(&a), 0);
    }

    #[test]
    fn distance_correct_order() {
        let a = Multihash::random(Hash::SHA2256);
        let b = Multihash::random(Hash::SHA2256);
        assert!(a.distance_with(&a) < b.distance_with(&a));
        assert!(a.distance_with(&b) > b.distance_with(&b));
    }
}
