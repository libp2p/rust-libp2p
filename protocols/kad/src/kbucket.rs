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
use std::num::NonZeroUsize;
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
    /// The timeout when trying to reach the youngest node after which we consider it unresponsive.
    unresponsive_timeout: Duration,
}

/// An individual table that stores peers or values.
#[derive(Debug, Clone)]
struct KBucket<TPeerId, TVal> {
    /// Nodes are always ordered from oldest to newest. The nodes we are connected to are always
    /// all on top (ie. have higher indices) of the nodes we are not connected to.
    nodes: ArrayVec<[Node<TPeerId, TVal>; MAX_NODES_PER_BUCKET]>,

    /// Index in `nodes` over which all nodes are connected. Must always be <= to the length
    /// of `nodes`.
    first_connected_pos: usize,

    /// Node received when the bucket was full. Will be added to the list if the youngest node
    /// doesn't respond in time to our reach attempt.
    pending_node: Option<PendingNode<TPeerId, TVal>>,
}

/// State of the pending node.
#[derive(Debug, Clone)]
struct PendingNode<TPeerId, TVal> {
    /// Node to insert.
    node: Node<TPeerId, TVal>,

    /// If true, we are connected to the pending node.
    connected: bool,

    /// When the pending node will replace an existing node, provided that the youngest node
    /// doesn't become responsive before.
    replace: Instant,
}

/// A single node in a k-bucket.
#[derive(Debug, Clone)]
struct Node<TPeerId, TVal> {
    /// Id of the node.
    id: TPeerId,
    /// Value associated to it.
    value: TVal,
}

/// Trait that must be implemented on types that can be used as an identifier in a k-bucket.
///
/// If `TOther` is not the same as `Self`, it represents an entry already in the k-buckets that
/// `Self` can compare against.
pub trait KBucketsPeerId<TOther = Self>: PartialEq<TOther> {
    /// Computes the XOR of this value and another one. The lower the closer.
    fn distance_with(&self, other: &TOther) -> u32;

    /// Returns then number of bits that are necessary to store the distance between peer IDs.
    /// Used for pre-allocations.
    fn max_distance() -> NonZeroUsize;
}

impl KBucketsPeerId for PeerId {
    fn distance_with(&self, other: &Self) -> u32 {
        <Multihash as KBucketsPeerId<Multihash>>::distance_with(self.as_ref(), other.as_ref())
    }

    fn max_distance() -> NonZeroUsize {
        <Multihash as KBucketsPeerId>::max_distance()
    }
}

impl KBucketsPeerId<PeerId> for Multihash {
    fn distance_with(&self, other: &PeerId) -> u32 {
        <Multihash as KBucketsPeerId<Multihash>>::distance_with(self, other.as_ref())
    }

    fn max_distance() -> NonZeroUsize {
        <PeerId as KBucketsPeerId>::max_distance()
    }
}

impl KBucketsPeerId for Multihash {
    fn distance_with(&self, other: &Self) -> u32 {
        // Note that we don't compare the hash functions because there's no chance of collision
        // of the same value hashed with two different hash functions.
        let my_hash = U512::from(self.digest());
        let other_hash = U512::from(other.digest());
        let xor = my_hash ^ other_hash;
        512 - xor.leading_zeros()
    }

    fn max_distance() -> NonZeroUsize {
        NonZeroUsize::new(512).expect("512 is not zero; QED")
    }
}

impl<A, B> KBucketsPeerId for (A, B)
where
    A: KBucketsPeerId + PartialEq,
    B: KBucketsPeerId + PartialEq,
{
    fn distance_with(&self, other: &(A, B)) -> u32 {
        A::distance_with(&self.0, &other.0) + B::distance_with(&self.1, &other.1)
    }

    fn max_distance() -> NonZeroUsize {
        let n = <A as KBucketsPeerId<A>>::max_distance().get()
            .saturating_add(<B as KBucketsPeerId<B>>::max_distance().get());
        NonZeroUsize::new(n).expect("Saturating-add of two non-zeros can't be zero; QED")
    }
}

impl<'a, T> KBucketsPeerId for &'a T
where
    T: KBucketsPeerId,
{
    fn distance_with(&self, other: &&'a T) -> u32 {
        T::distance_with(*self, *other)
    }

    fn max_distance() -> NonZeroUsize {
        <T as KBucketsPeerId>::max_distance()
    }
}

impl<TPeerId, TVal> KBucketsTable<TPeerId, TVal>
where
    TPeerId: KBucketsPeerId + Clone,
{
    /// Builds a new routing table.
    pub fn new(my_id: TPeerId, unresponsive_timeout: Duration) -> Self {
        KBucketsTable {
            my_id,
            tables: (0..TPeerId::max_distance().get())
                .map(|_| KBucket {
                    nodes: ArrayVec::new(),
                    first_connected_pos: 0,
                    pending_node: None,
                })
                .collect(),
            unresponsive_timeout,
        }
    }

    /// Returns the ID of the local node.
    pub fn my_id(&self) -> &TPeerId {
        &self.my_id
    }

    /// Returns the id of the bucket that should contain the peer with the given ID.
    ///
    /// Returns `None` if out of range, which happens if `id` is the same as the local peer id.
    fn bucket_num(&self, id: &TPeerId) -> Option<usize> {
        (self.my_id.distance_with(id) as usize).checked_sub(1)
    }

    /// Returns an object containing the state of the given entry.
    pub fn entry<'a>(&'a mut self, peer_id: &'a TPeerId) -> Entry<'a, TPeerId, TVal> {
        let bucket_num = if let Some(num) = self.bucket_num(peer_id) {
            num
        } else {
            return Entry::SelfEntry;
        };

        // Update the pending node state.
        // TODO: must be reported to the user somehow, in a non-annoying API
        if let Some(pending) = self.tables[bucket_num].pending_node.take() {
            if pending.replace < Instant::now() {
                let table = &mut self.tables[bucket_num];
                let first_connected_pos = &mut table.first_connected_pos;
                // If all the nodes in the bucket are connected, then there shouldn't be any
                // pending node.
                debug_assert!(*first_connected_pos >= 1);
                table.nodes.remove(0);
                if pending.connected {
                    *first_connected_pos -= 1;
                    table.nodes.insert(*first_connected_pos, pending.node);
                } else {
                    table.nodes.insert(*first_connected_pos - 1, pending.node);
                }
            } else {
                self.tables[bucket_num].pending_node = Some(pending);
            }
        }

        // Try to find the node in the bucket.
        if let Some(pos) = self.tables[bucket_num].nodes.iter().position(|p| p.id == *peer_id) {
            if pos >= self.tables[bucket_num].first_connected_pos {
                Entry::InKbucketConnected(EntryInKbucketConn {
                    parent: self,
                    peer_id,
                })

            } else {
                Entry::InKbucketDisconnected(EntryInKbucketDisc {
                    parent: self,
                    peer_id,
                })
            }

        } else if self.tables[bucket_num].pending_node.as_ref().map(|p| p.node.id == *peer_id).unwrap_or(false) {
            // Node is pending.
            if self.tables[bucket_num].pending_node.as_ref().map(|p| p.connected).unwrap_or(false) {
                Entry::InKbucketConnectedPending(EntryInKbucketConnPending {
                    parent: self,
                    peer_id,
                })
            } else {
                Entry::InKbucketDisconnectedPending(EntryInKbucketDiscPending {
                    parent: self,
                    peer_id,
                })
            }

        } else {
            Entry::NotInKbucket(EntryNotInKbucket {
                parent: self,
                peer_id,
            })
        }
    }

    /// Returns an iterator to all the peer IDs in the bucket, without the pending nodes.
    pub fn entries_not_pending(&self) -> impl Iterator<Item = (&TPeerId, &TVal)> {
        self.tables
            .iter()
            .flat_map(|table| table.nodes.iter())
            .map(|node| (&node.id, &node.value))
    }

    /// Returns an iterator to all the buckets of this table.
    ///
    /// Ordered by proximity to the local node. Closest bucket (with max. one node in it) comes
    /// first.
    pub fn buckets(&mut self) -> BucketsIter<'_, TPeerId, TVal> {
        BucketsIter(self.tables.iter_mut(), self.unresponsive_timeout)
    }

    /// Finds the nodes closest to `id`, ordered by distance.
    ///
    /// Pending nodes are ignored.
    pub fn find_closest(&mut self, id: &impl KBucketsPeerId<TPeerId>) -> VecIntoIter<TPeerId> {
        // TODO: optimize
        let mut out = Vec::new();
        for table in self.tables.iter_mut() {
            for node in table.nodes.iter() {
                out.push(node.id.clone());
            }

            // TODO: this code that handles the pending_node should normally be shared with
            //       the one in `entry()`; however right now there's no mechanism to notify the
            //       user when a pending node has been inserted in the table, and thus we need to
            //       rework this pending node handling code anyway; when that is being done, we
            //       should rewrite this code properly
            if let Some(ref pending) = table.pending_node {
                if pending.replace <= Instant::now() && pending.connected {
                    out.pop();
                    out.push(pending.node.id.clone());
                }
            }
        }
        out.sort_by(|a, b| id.distance_with(a).cmp(&id.distance_with(b)));
        out.into_iter()
    }
}

/// Represents an entry or a potential entry in the k-buckets.
pub enum Entry<'a, TPeerId, TVal> {
    /// Entry in a k-bucket that we're connected to.
    InKbucketConnected(EntryInKbucketConn<'a, TPeerId, TVal>),
    /// Entry pending waiting for a free slot to enter a k-bucket. We're connected to it.
    InKbucketConnectedPending(EntryInKbucketConnPending<'a, TPeerId, TVal>),
    /// Entry in a k-bucket but that we're not connected to.
    InKbucketDisconnected(EntryInKbucketDisc<'a, TPeerId, TVal>),
    /// Entry pending waiting for a free slot to enter a k-bucket. We're not connected to it.
    InKbucketDisconnectedPending(EntryInKbucketDiscPending<'a, TPeerId, TVal>),
    /// Entry is not present in any k-bucket.
    NotInKbucket(EntryNotInKbucket<'a, TPeerId, TVal>),
    /// Entry is the local peer ID.
    SelfEntry,
}

impl<'a, TPeerId, TVal> Entry<'a, TPeerId, TVal>
where
    TPeerId: KBucketsPeerId + Clone,
{
    /// Returns the value associated to the entry in the bucket, including if the node is pending.
    pub fn value(&mut self) -> Option<&mut TVal> {
        match self {
            Entry::InKbucketConnected(entry) => Some(entry.value()),
            Entry::InKbucketConnectedPending(entry) => Some(entry.value()),
            Entry::InKbucketDisconnected(entry) => Some(entry.value()),
            Entry::InKbucketDisconnectedPending(entry) => Some(entry.value()),
            Entry::NotInKbucket(_entry) => None,
            Entry::SelfEntry => None,
        }
    }

    /// Returns the value associated to the entry in the bucket.
    pub fn value_not_pending(&mut self) -> Option<&mut TVal> {
        match self {
            Entry::InKbucketConnected(entry) => Some(entry.value()),
            Entry::InKbucketConnectedPending(_entry) => None,
            Entry::InKbucketDisconnected(entry) => Some(entry.value()),
            Entry::InKbucketDisconnectedPending(_entry) => None,
            Entry::NotInKbucket(_entry) => None,
            Entry::SelfEntry => None,
        }
    }
}

/// Represents an entry in a k-bucket.
pub struct EntryInKbucketConn<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a TPeerId,
}

impl<'a, TPeerId, TVal> EntryInKbucketConn<'a, TPeerId, TVal>
where
    TPeerId: KBucketsPeerId + Clone,
{
    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryInKbucketConn if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        let peer_id = self.peer_id;
        &mut table.nodes.iter_mut()
            .find(move |p| p.id == *peer_id)
            .expect("We can only build a EntryInKbucketConn if we know that the peer is in its \
                     bucket; QED")
            .value
    }

    /// Reports that we are now disconnected from the given node.
    ///
    /// This moves the node down in its bucket. There are two possible outcomes:
    ///
    /// - Either we had a pending node which replaces the current node. `Replaced` is returned.
    /// - Or we had no pending node, and the current node is kept. `Kept` is returned.
    ///
    pub fn set_disconnected(self) -> SetDisconnectedOutcome<'a, TPeerId, TVal> {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryInKbucketConn if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        let peer_id = self.peer_id;
        let pos = table.nodes.iter().position(move |elem| elem.id == *peer_id)
            .expect("we can only build a EntryInKbucketConn if the node is in its bucket; QED");
        debug_assert!(table.first_connected_pos <= pos);

        // We replace it with the pending node, if any.
        if let Some(pending) = table.pending_node.take() {
            if pending.connected {
                let removed = table.nodes.remove(pos);
                let ret = SetDisconnectedOutcome::Replaced {
                    replacement: pending.node.id.clone(),
                    old_val: removed.value,
                };
                table.nodes.insert(table.first_connected_pos, pending.node);
                return ret;
            } else {
                table.pending_node = Some(pending);
            }
        }

        // Move the node in the bucket.
        if pos != table.first_connected_pos {
            let elem = table.nodes.remove(pos);
            table.nodes.insert(table.first_connected_pos, elem);
        }
        table.first_connected_pos += 1;

        // And return a EntryInKbucketDisc.
        SetDisconnectedOutcome::Kept(EntryInKbucketDisc {
            parent: self.parent,
            peer_id: self.peer_id,
        })
    }
}

/// Outcome of calling `set_disconnected`.
#[must_use]
pub enum SetDisconnectedOutcome<'a, TPeerId, TVal> {
    /// Node is kept in the bucket.
    Kept(EntryInKbucketDisc<'a, TPeerId, TVal>),
    /// Node is pushed out of the bucket.
    Replaced {
        /// Node that replaced the node.
        // TODO: could be a EntryInKbucketConn, but we have borrow issues with the new peer id
        replacement: TPeerId,
        /// Value os the node that has been pushed out.
        old_val: TVal,
    },
}

/// Represents an entry waiting for a slot to be available in its k-bucket.
pub struct EntryInKbucketConnPending<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a TPeerId,
}

impl<'a, TPeerId, TVal> EntryInKbucketConnPending<'a, TPeerId, TVal>
where
    TPeerId: KBucketsPeerId + Clone,
{
    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryInKbucketConnPending if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        assert!(table.pending_node.as_ref().map(|n| &n.node.id) == Some(self.peer_id));
        &mut table.pending_node
            .as_mut()
            .expect("we can only build a EntryInKbucketConnPending if the node is pending; QED")
            .node.value
    }

    /// Reports that we are now disconnected from the given node.
    pub fn set_disconnected(self) -> EntryInKbucketDiscPending<'a, TPeerId, TVal> {
        {
            let table = {
                let num = self.parent.bucket_num(&self.peer_id)
                    .expect("we can only build a EntryInKbucketConnPending if we know of a bucket; QED");
                &mut self.parent.tables[num]
            };

            let mut pending = table.pending_node.as_mut()
                .expect("we can only build a EntryInKbucketConnPending if there's a pending node; QED");
            debug_assert!(pending.connected);
            pending.connected = false;
        }

        EntryInKbucketDiscPending {
            parent: self.parent,
            peer_id: self.peer_id,
        }
    }
}

/// Represents an entry waiting for a slot to be available in its k-bucket.
pub struct EntryInKbucketDiscPending<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a TPeerId,
}

impl<'a, TPeerId, TVal> EntryInKbucketDiscPending<'a, TPeerId, TVal>
where
    TPeerId: KBucketsPeerId + Clone,
{
    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryInKbucketDiscPending if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        assert!(table.pending_node.as_ref().map(|n| &n.node.id) == Some(self.peer_id));
        &mut table.pending_node
            .as_mut()
            .expect("we can only build a EntryInKbucketDiscPending if the node is pending; QED")
            .node.value
    }

    /// Reports that we are now connected to the given node.
    pub fn set_connected(self) -> EntryInKbucketConnPending<'a, TPeerId, TVal> {
        {
            let table = {
                let num = self.parent.bucket_num(&self.peer_id)
                    .expect("we can only build a EntryInKbucketDiscPending if we know of a bucket; QED");
                &mut self.parent.tables[num]
            };

            let mut pending = table.pending_node.as_mut()
                .expect("we can only build a EntryInKbucketDiscPending if there's a pending node; QED");
            debug_assert!(!pending.connected);
            pending.connected = true;
        }

        EntryInKbucketConnPending {
            parent: self.parent,
            peer_id: self.peer_id,
        }
    }
}

/// Represents an entry in a k-bucket.
pub struct EntryInKbucketDisc<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a TPeerId,
}

impl<'a, TPeerId, TVal> EntryInKbucketDisc<'a, TPeerId, TVal>
where
    TPeerId: KBucketsPeerId + Clone,
{
    /// Returns the value associated to the entry in the bucket.
    pub fn value(&mut self) -> &mut TVal {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryInKbucketDisc if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        let peer_id = self.peer_id;
        &mut table.nodes.iter_mut()
            .find(move |p| p.id == *peer_id)
            .expect("We can only build a EntryInKbucketDisc if we know that the peer is in its \
                     bucket; QED")
            .value
    }

    /// Sets the node as connected. This moves the entry in the bucket.
    pub fn set_connected(self) -> EntryInKbucketConn<'a, TPeerId, TVal> {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryInKbucketDisc if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        let pos = {
            let peer_id = self.peer_id;
            table.nodes.iter().position(move |p| p.id == *peer_id)
                .expect("We can only build a EntryInKbucketDisc if we know that the peer is in \
                         its bucket; QED")
        };

        // If we are the youngest node, we are now connected, which means that we have to drop the
        // pending node.
        // Note that it is theoretically possible that the replacement should have occurred between
        // the moment when we build the `EntryInKbucketConn` and the moment when we call
        // `set_connected`, but we don't take that into account.
        if pos == 0 {
            table.pending_node = None;
        }

        debug_assert!(pos < table.first_connected_pos);
        table.first_connected_pos -= 1;
        if pos != table.first_connected_pos {
            let entry = table.nodes.remove(pos);
            table.nodes.insert(table.first_connected_pos, entry);
        }

        // There shouldn't be a pending node if all slots are full of connected nodes.
        debug_assert!(!(table.first_connected_pos == 0 && table.pending_node.is_some()));

        EntryInKbucketConn {
            parent: self.parent,
            peer_id: self.peer_id,
        }
    }
}

/// Represents an entry not in any k-bucket.
pub struct EntryNotInKbucket<'a, TPeerId, TVal> {
    parent: &'a mut KBucketsTable<TPeerId, TVal>,
    peer_id: &'a TPeerId,
}

impl<'a, TPeerId, TVal> EntryNotInKbucket<'a, TPeerId, TVal>
where
    TPeerId: KBucketsPeerId + Clone,
{
    /// Inserts the node as connected, if possible.
    pub fn insert_connected(self, value: TVal) -> InsertOutcome<TPeerId> {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryNotInKbucket if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        if table.nodes.is_full() {
            if table.first_connected_pos == 0 || table.pending_node.is_some() {
                InsertOutcome::Full
            } else {
                table.pending_node = Some(PendingNode {
                    node: Node { id: self.peer_id.clone(), value },
                    replace: Instant::now() + self.parent.unresponsive_timeout,
                    connected: true,
                });
                InsertOutcome::Pending {
                    to_ping: table.nodes[0].id.clone()
                }
            }
        } else {
            table.nodes.insert(table.first_connected_pos, Node {
                id: self.peer_id.clone(),
                value,
            });
            InsertOutcome::Inserted
        }
    }

    /// Inserts the node as disconnected, if possible.
    ///
    /// > **Note**: This function will never return `Pending`. If the bucket is full, we simply
    /// >           do nothing.
    pub fn insert_disconnected(self, value: TVal) -> InsertOutcome<TPeerId> {
        let table = {
            let num = self.parent.bucket_num(&self.peer_id)
                .expect("we can only build a EntryNotInKbucket if we know of a bucket; QED");
            &mut self.parent.tables[num]
        };

        if table.nodes.is_full() {
            InsertOutcome::Full
        } else {
            table.nodes.insert(table.first_connected_pos, Node {
                id: self.peer_id.clone(),
                value,
            });
            table.first_connected_pos += 1;
            InsertOutcome::Inserted
        }
    }
}

/// Outcome of calling `insert`.
#[must_use]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum InsertOutcome<TPeerId> {
    /// The entry has been successfully inserted.
    Inserted,
    /// The entry has been inserted as a pending node.
    Pending {
        /// We have to try connect to the returned node.
        to_ping: TPeerId,
    },
    /// The entry was not inserted because the bucket was full of connected nodes.
    Full,
}

/// Iterator giving access to a bucket.
pub struct BucketsIter<'a, TPeerId, TVal>(SliceIterMut<'a, KBucket<TPeerId, TVal>>, Duration);

impl<'a, TPeerId, TVal> Iterator for BucketsIter<'a, TPeerId, TVal> {
    type Item = Bucket<'a, TPeerId, TVal>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|bucket| {
            Bucket(bucket)
        })
    }

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
    use crate::kbucket::{Entry, InsertOutcome, KBucketsPeerId, KBucketsTable, MAX_NODES_PER_BUCKET};
    use multihash::{Multihash, Hash};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn basic_closest() {
        let my_id = Multihash::random(Hash::SHA2256);
        let other_id = Multihash::random(Hash::SHA2256);

        let mut table = KBucketsTable::<_, ()>::new(my_id, Duration::from_secs(5));
        if let Entry::NotInKbucket(entry) = table.entry(&other_id) {
            match entry.insert_connected(()) {
                InsertOutcome::Inserted => (),
                _ => panic!()
            }
        } else {
            panic!()
        }

        let res = table.find_closest(&other_id).collect::<Vec<_>>();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], other_id);
    }

    #[test]
    fn update_local_id_fails() {
        let my_id = Multihash::random(Hash::SHA2256);

        let mut table = KBucketsTable::<_, ()>::new(my_id.clone(), Duration::from_secs(5));
        match table.entry(&my_id) {
            Entry::SelfEntry => (),
            _ => panic!(),
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
            if let Entry::NotInKbucket(entry) = table.entry(&id) {
                match entry.insert_disconnected(()) {
                    InsertOutcome::Inserted => (),
                    _ => panic!()
                }
            } else {
                panic!()
            }
            assert_eq!(table.buckets().nth(255).unwrap().num_entries(), num + 1);
        }

        assert_eq!(
            table.buckets().nth(255).unwrap().num_entries(),
            MAX_NODES_PER_BUCKET
        );
        assert!(!table.buckets().nth(255).unwrap().has_pending());
        if let Entry::NotInKbucket(entry) = table.entry(&fill_ids.remove(0)) {
            match entry.insert_connected(()) {
                InsertOutcome::Pending { ref to_ping } if *to_ping == first_node => (),
                _ => panic!()
            }
        } else {
            panic!()
        }

        assert_eq!(
            table.buckets().nth(255).unwrap().num_entries(),
            MAX_NODES_PER_BUCKET
        );
        assert!(table.buckets().nth(255).unwrap().has_pending());
        if let Entry::NotInKbucket(entry) = table.entry(&fill_ids.remove(0)) {
            match entry.insert_connected(()) {
                InsertOutcome::Full => (),
                _ => panic!()
            }
        } else {
            panic!()
        }

        thread::sleep(Duration::from_secs(2));
        assert!(!table.buckets().nth(255).unwrap().has_pending());
        if let Entry::NotInKbucket(entry) = table.entry(&fill_ids.remove(0)) {
            match entry.insert_connected(()) {
                InsertOutcome::Pending { ref to_ping } if *to_ping == second_node => (),
                _ => panic!()
            }
        } else {
            panic!()
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
