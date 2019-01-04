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
use multihash::Multihash;
use std::mem;
use std::slice::IterMut as SliceIterMut;
use std::time::{Duration, Instant};
use std::vec::IntoIter as VecIntoIter;

/// Maximum number of nodes in a bucket.
pub const MAX_NODES_PER_BUCKET: usize = 20;

/// Table of k-buckets.
#[derive(Debug, Clone)]
pub struct KBucketsTable<Id, Val> {
    /// Peer ID of the local node.
    my_id: Id,
     /// The actual tables that store peers or values.
    tables: Vec<KBucket<Id, Val>>,
     // The timeout when pinging the first node after which we consider it unresponsive.
    ping_timeout: Duration,
}

/// An individual table that stores peers or values.
#[derive(Debug, Clone)]
struct KBucket<Id, Val> {
    /// Nodes are always ordered from oldest to newest.
    /// Note that we will very often move elements to the end of this. No benchmarking has been
    /// performed, but it is very likely that a `ArrayVec` is the most performant data structure.
    nodes: ArrayVec<[Node<Id, Val>; MAX_NODES_PER_BUCKET]>,

    /// Node received when the bucket was full. Will be added to the list if the first node doesn't
    /// respond in time to our ping. The second element is the time when the pending node was added.
     /// If it is too old we drop the first node and add the pending node to the
    /// end of the list.
    pending_node: Option<(Node<Id, Val>, Instant)>,

    /// Last time this bucket was updated.
    last_update: Instant,
}

#[derive(Debug, Clone)]
struct Node<Id, Val> {
    id: Id,
    value: Val,
}

impl<Id, Val> KBucket<Id, Val> {
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
pub trait KBucketsPeerId: Eq + Clone {
    /// Computes the XOR of this value and another one. The lower the closer.
    fn distance_with(&self, other: &Self) -> u32;

    /// Returns then number of bits that are necessary to store the distance between peer IDs.
    /// Used for pre-allocations.
    ///
    /// > **Note**: Returning 0 would lead to a panic.
    fn max_distance() -> usize;
}

impl KBucketsPeerId for Multihash {
    #[inline]
    fn distance_with(&self, other: &Self) -> u32 {
        // Note that we don't compare the hash functions because there's no chance of collision
        // of the same value hashed with two different hash functions.
        let my_hash = U512::from(self.digest());
        let other_hash = U512::from(other.digest());
        let xor = my_hash ^ other_hash;
        xor.leading_zeros()
    }

    #[inline]
    fn max_distance() -> usize {
        512
    }
}

impl<Id, Val> KBucketsTable<Id, Val>
where
    Id: KBucketsPeerId,
{
    /// Builds a new routing table.
    pub fn new(my_id: Id, ping_timeout: Duration) -> Self {
        KBucketsTable {
            my_id: my_id,
            tables: (0..Id::max_distance())
                .map(|_| KBucket {
                    nodes: ArrayVec::new(),
                    pending_node: None,
                    last_update: Instant::now(),
                })
                .collect(),
            ping_timeout: ping_timeout,
        }
    }

    // Returns the id of the bucket that should contain the peer with the given ID.
    //
    // Returns `None` if out of range, which happens if `id` is the same as the local peer id.
    #[inline]
    fn bucket_num(&self, id: &Id) -> Option<usize> {
        (Id::max_distance() - 1).checked_sub(self.my_id.distance_with(id) as usize)
    }

    /// Returns an iterator to all the buckets of this table.
    ///
    /// Ordered by proximity to the local node. Closest bucket (with max. one node in it) comes
    /// first.
    #[inline]
    pub fn buckets(&mut self) -> BucketsIter<Id, Val> {
        BucketsIter(self.tables.iter_mut(), self.ping_timeout)
    }

    /// Returns the ID of the local node.
    #[inline]
    pub fn my_id(&self) -> &Id {
        &self.my_id
    }

    /// Finds the `num` nodes closest to `id`, ordered by distance.
    pub fn find_closest(&mut self, id: &Id) -> VecIntoIter<Id>
    where
        Id: Clone,
    {
        // TODO: optimize
        let mut out = Vec::new();
        for table in self.tables.iter_mut() {
            table.flush(self.ping_timeout);
            if table.last_update.elapsed() > self.ping_timeout {
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
    pub fn find_closest_with_self(&mut self, id: &Id) -> VecIntoIter<Id>
    where
        Id: Clone,
    {
        // TODO: optimize
        let mut intermediate: Vec<_> = self.find_closest(&id).collect();
        if let Some(pos) = intermediate
            .iter()
            .position(|e| e.distance_with(&id) >= self.my_id.distance_with(&id))
        {
            if intermediate[pos] != self.my_id {
                intermediate.insert(pos, self.my_id.clone());
            }
        } else {
            intermediate.push(self.my_id.clone());
        }
        intermediate.into_iter()
    }

    /// Marks the node as "most recent" in its bucket and modifies the value associated to it.
    /// This function should be called whenever we receive a communication from a node.
    pub fn update(&mut self, id: Id, value: Val) -> UpdateOutcome<Id, Val> {
        let table = match self.bucket_num(&id) {
            Some(n) => &mut self.tables[n],
            None => return UpdateOutcome::FailSelfUpdate,
        };

        table.flush(self.ping_timeout);

        if let Some(pos) = table.nodes.iter().position(|n| n.id == id) {
            // Node is already in the bucket.
            let mut existing = table.nodes.remove(pos);
            let old_val = mem::replace(&mut existing.value, value);
            if pos == 0 {
                // If it's the first node of the bucket that we update, then we drop the node that
                // was waiting for a ping.
                table.nodes.truncate(MAX_NODES_PER_BUCKET - 1);
                table.pending_node = None;
            }
            table.nodes.push(existing);
            table.last_update = Instant::now();
            UpdateOutcome::Refreshed(old_val)
        } else if table.nodes.len() < MAX_NODES_PER_BUCKET {
            // Node not yet in the bucket, but there's plenty of space.
            table.nodes.push(Node {
                id: id,
                value: value,
            });
            table.last_update = Instant::now();
            UpdateOutcome::Added
        } else {
            // Not enough space to put the node, but we can add it to the end as "pending". We
            // then need to tell the caller that we want it to ping the node at the top of the
            // list.
            if table.pending_node.is_none() {
                table.pending_node = Some((
                    Node {
                        id: id,
                        value: value,
                    },
                    Instant::now(),
                ));
                UpdateOutcome::NeedPing(table.nodes[0].id.clone())
            } else {
                UpdateOutcome::Discarded
            }
        }
    }
}

/// Return value of the `update()` method.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[must_use]
pub enum UpdateOutcome<Id, Val> {
    /// The node has been added to the bucket.
    Added,
    /// The node was already in the bucket and has been refreshed.
    Refreshed(Val),
    /// The node wasn't added. Instead we need to ping the node passed as parameter, and call
    /// `update` if it responds.
    NeedPing(Id),
    /// The node wasn't added at all because a node was already pending.
    Discarded,
    /// Tried to update the local peer ID. This is an invalid operation.
    FailSelfUpdate,
}

/// Iterator giving access to a bucket.
pub struct BucketsIter<'a, Id: 'a, Val: 'a>(SliceIterMut<'a, KBucket<Id, Val>>, Duration);

impl<'a, Id: 'a, Val: 'a> Iterator for BucketsIter<'a, Id, Val> {
    type Item = Bucket<'a, Id, Val>;

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

impl<'a, Id: 'a, Val: 'a> ExactSizeIterator for BucketsIter<'a, Id, Val> {}

/// Access to a bucket.
pub struct Bucket<'a, Id: 'a, Val: 'a>(&'a mut KBucket<Id, Val>);

impl<'a, Id: 'a, Val: 'a> Bucket<'a, Id, Val> {
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
    pub fn last_update(&self) -> Instant {
        self.0.last_update.clone()
    }
}

#[cfg(test)]
mod tests {
    extern crate rand;
    use self::rand::random;
    use kbucket::{KBucketsTable, UpdateOutcome, MAX_NODES_PER_BUCKET};
    use multihash::Multihash;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn basic_closest() {
        let my_id = {
            let mut bytes = vec![random(); 34];
            bytes[0] = 18;
            bytes[1] = 32;
            Multihash::from_bytes(bytes.clone()).expect(&format!(
                "creating `my_id` Multihash from bytes {:#?} failed",
                bytes
            ))
        };

        let other_id = {
            let mut bytes = vec![random(); 34];
            bytes[0] = 18;
            bytes[1] = 32;
            Multihash::from_bytes(bytes.clone()).expect(&format!(
                "creating `other_id` Multihash from bytes {:#?} failed",
                bytes
            ))
        };

        let mut table = KBucketsTable::new(my_id, Duration::from_secs(5));
        let _ = table.update(other_id.clone(), ());

        let res = table.find_closest(&other_id).collect::<Vec<_>>();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], other_id);
    }

    #[test]
    fn update_local_id_fails() {
        let my_id = {
            let mut bytes = vec![random(); 34];
            bytes[0] = 18;
            bytes[1] = 32;
            Multihash::from_bytes(bytes).unwrap()
        };

        let mut table = KBucketsTable::new(my_id.clone(), Duration::from_secs(5));
        match table.update(my_id, ()) {
            UpdateOutcome::FailSelfUpdate => (),
            _ => panic!(),
        }
    }

    #[test]
    fn update_time_last_refresh() {
        let my_id = {
            let mut bytes = vec![random(); 34];
            bytes[0] = 18;
            bytes[1] = 32;
            Multihash::from_bytes(bytes).unwrap()
        };

        // Generate some other IDs varying by just one bit.
        let other_ids = (0..random::<usize>() % 20)
            .map(|_| {
                let bit_num = random::<usize>() % 256;
                let mut id = my_id.as_bytes().to_vec().clone();
                id[33 - (bit_num / 8)] ^= 1 << (bit_num % 8);
                (Multihash::from_bytes(id).unwrap(), bit_num)
            })
            .collect::<Vec<_>>();

        let mut table = KBucketsTable::new(my_id, Duration::from_secs(5));
        let before_update = table.buckets().map(|b| b.last_update()).collect::<Vec<_>>();

        thread::sleep(Duration::from_secs(2));
        for &(ref id, _) in &other_ids {
            let _ = table.update(id.clone(), ());
        }

        let after_update = table.buckets().map(|b| b.last_update()).collect::<Vec<_>>();

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
        let my_id = {
            let mut bytes = vec![random(); 34];
            bytes[0] = 18;
            bytes[1] = 32;
            Multihash::from_bytes(bytes).unwrap()
        };

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

        let mut table = KBucketsTable::new(my_id.clone(), Duration::from_secs(1));

        for (num, id) in fill_ids.drain(..MAX_NODES_PER_BUCKET).enumerate() {
            assert_eq!(table.update(id, ()), UpdateOutcome::Added);
            assert_eq!(table.buckets().nth(255).unwrap().num_entries(), num + 1);
        }

        assert_eq!(
            table.buckets().nth(255).unwrap().num_entries(),
            MAX_NODES_PER_BUCKET
        );
        assert!(!table.buckets().nth(255).unwrap().has_pending());
        assert_eq!(
            table.update(fill_ids.remove(0), ()),
            UpdateOutcome::NeedPing(first_node)
        );

        assert_eq!(
            table.buckets().nth(255).unwrap().num_entries(),
            MAX_NODES_PER_BUCKET
        );
        assert!(table.buckets().nth(255).unwrap().has_pending());
        assert_eq!(
            table.update(fill_ids.remove(0), ()),
            UpdateOutcome::Discarded
        );

        thread::sleep(Duration::from_secs(2));
        assert!(!table.buckets().nth(255).unwrap().has_pending());
        assert_eq!(
            table.update(fill_ids.remove(0), ()),
            UpdateOutcome::NeedPing(second_node)
        );
    }
}
