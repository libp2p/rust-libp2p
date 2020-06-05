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

//! The internal API for a single `KBucket` in a `KBucketsTable`.
//!
//! > **Note**: Uniqueness of entries w.r.t. a `Key` in a `KBucket` is not
//! > checked in this module. This is an invariant that must hold across all
//! > buckets in a `KBucketsTable` and hence is enforced by the public API
//! > of the `KBucketsTable` and in particular the public `Entry` API.

use super::*;
pub use crate::kbucket::sub_bucket::{Node, NodeStatus, PendingNode, Position, SubBucket};
use crate::kbucket::swamp::Swamp;
use crate::kbucket::weighted::Weighted;
pub use crate::{K_VALUE, W_VALUE};
use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct KBucket<TKey, TVal> {
    swamp: Swamp<TKey, TVal>,
    weighted: Weighted<TKey, TVal>,
    pending_timeout: Duration,
}

/*
// /// A `KBucket` is a list of up to `K_VALUE` keys and associated values,
// /// ordered from least-recently connected to most-recently connected.
// #[derive(Debug, Clone)]
// pub struct KBucket<TKey, TVal> {
//     /// The nodes contained in the bucket.
//     nodes: ArrayVec<[Node<TKey, TVal>; K_VALUE.get()]>,
//
//     /// The position (index) in `nodes` that marks the first connected node.
//     ///
//     /// Since the entries in `nodes` are ordered from least-recently connected to
//     /// most-recently connected, all entries above this index are also considered
//     /// connected, i.e. the range `[0, first_connected_pos)` marks the sub-list of entries
//     /// that are considered disconnected and the range
//     /// `[first_connected_pos, K_VALUE)` marks sub-list of entries that are
//     /// considered connected.
//     ///
//     /// `None` indicates that there are no connected entries in the bucket, i.e.
//     /// the bucket is either empty, or contains only entries for peers that are
//     /// considered disconnected.
//     first_connected_pos: Option<usize>,
//
//     /// A node that is pending to be inserted into a full bucket, should the
//     /// least-recently connected (and currently disconnected) node not be
//     /// marked as connected within `unresponsive_timeout`.
//     pending: Option<PendingNode<TKey, TVal>>,
//
//     /// The timeout window before a new pending node is eligible for insertion,
//     /// if the least-recently connected node is not updated as being connected
//     /// in the meantime.
//     pending_timeout: Duration
// }
*/

/// The result of inserting an entry into a bucket.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertResult<TKey> {
    /// The entry has been successfully inserted.
    Inserted,
    /// The entry is pending insertion because the relevant bucket is currently full.
    /// The entry is inserted after a timeout elapsed, if the status of the
    /// least-recently connected (and currently disconnected) node in the bucket
    /// is not updated before the timeout expires.
    Pending {
        /// The key of the least-recently connected entry that is currently considered
        /// disconnected and whose corresponding peer should be checked for connectivity
        /// in order to prevent it from being evicted. If connectivity to the peer is
        /// re-established, the corresponding entry should be updated with
        /// [`NodeStatus::Connected`].
        disconnected: TKey,
    },
    /// The entry was not inserted because the relevant bucket is full.
    Full,
}

impl<Key> std::fmt::Display for InsertResult<Key> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let descritpion = match self {
            InsertResult::Inserted => "InsertResult::Inserted",
            InsertResult::Pending { .. } => "InsertResult::Pending",
            InsertResult::Full => "InsertResult::Full",
        };

        write!(f, "{}", descritpion)
    }
}

/// The result of applying a pending node to a bucket, possibly
/// replacing an existing node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPending<TKey, TVal> {
    /// The key of the inserted pending node.
    pub inserted: Node<TKey, TVal>,
    /// The node that has been evicted from the bucket to make room for the
    /// pending node, if any.
    pub evicted: Option<Node<TKey, TVal>>,
}

impl<TKey, TVal> KBucket<TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    /// Creates a new `KBucket` with the given timeout for pending entries.
    pub fn new(pending_timeout: Duration) -> Self {
        KBucket {
            swamp: Swamp::new(pending_timeout),
            weighted: Weighted::new(pending_timeout),
            pending_timeout,
        }
    }

    pub fn has_pending(&self) -> bool {
        self.pending_active(true) || self.pending_active(false)
    }

    /// Returns a reference to the pending node of the bucket, if there is any.
    // TODO: maybe return `impl Iterator`?
    fn pending(&self) -> Vec<&PendingNode<TKey, TVal>> {
        Iterator::chain(
            self.weighted.pending().into_iter(),
            self.swamp.pending().into_iter(),
        )
        .collect()
    }

    /// Returns a mutable reference to the pending node of the bucket, if there is any.
    pub fn pending_mut(&mut self, key: &TKey) -> Option<&mut PendingNode<TKey, TVal>> {
        Option::or(self.weighted.pending_mut(key), self.swamp.pending_mut(key))
    }

    /// Returns a reference to the pending node of the bucket, if there is any
    /// with a matching key.
    pub fn pending_ref(&self, key: &TKey) -> Option<&PendingNode<TKey, TVal>> {
        Option::or(self.weighted.pending_ref(key), self.swamp.pending_ref(key))
    }

    /// Updates the status of the pending node, if any.
    pub fn update_pending(&mut self, key: &TKey, status: NodeStatus) {
        if !self.weighted.update_pending(key, status) {
            if !self.swamp.update_pending(key, status) {
                println!(
                    "Didn't update pending node {:?} to {:?}",
                    key.as_ref(),
                    status
                )
            }
        }
    }

    /// Gets a mutable reference to the node identified by the given key.
    ///
    /// Returns `None` if the given key does not refer to a node in the
    /// bucket.
    pub fn get_mut(&mut self, key: &TKey) -> Option<&mut Node<TKey, TVal>> {
        Option::or(self.weighted.get_mut(key), self.swamp.get_mut(key))
    }

    /// Returns an iterator over the nodes in the bucket, together with their status.
    pub fn iter(&self) -> impl Iterator<Item = (&Node<TKey, TVal>, NodeStatus)> {
        Iterator::chain(self.weighted(), self.swamp())
    }

    /// Returns an iterator over the weighted nodes in the bucket, together with their status.
    pub fn weighted(&self) -> impl Iterator<Item = (&Node<TKey, TVal>, NodeStatus)> {
        self.weighted.iter()
    }

    /// Returns an iterator over the swamp nodes in the bucket, together with their status.
    pub fn swamp(&self) -> impl Iterator<Item = (&Node<TKey, TVal>, NodeStatus)> {
        self.swamp.iter()
    }

    /// Inserts the pending node into the bucket, if its timeout has elapsed,
    /// replacing the least-recently connected node.
    ///
    /// If a pending node has been inserted, its key is returned together with
    /// the node that was replaced. `None` indicates that the nodes in the
    /// bucket remained unchanged.
    pub fn apply_pending(&mut self) -> Vec<AppliedPending<TKey, TVal>> {
        Iterator::chain(
            self.weighted.apply_pending().into_iter(),
            self.swamp.apply_pending().into_iter(),
        )
        .collect()
    }

    /// Updates the status of the node referred to by the given key, if it is
    /// in the bucket.
    pub fn update(&mut self, key: &TKey, new_status: NodeStatus) {
        if !self.weighted.update(key, new_status) {
            if !self.swamp.update(key, new_status) {
                println!("Node {:?} wasn't updated to {:?}", key.as_ref(), new_status)
            }
        }
    }

    /// Inserts a new node into the bucket with the given status.
    ///
    /// The status of the node to insert determines the result as follows:
    ///
    ///   * `NodeStatus::Connected`: If the bucket is full and either all nodes are connected
    ///     or there is already a pending node, insertion fails with `InsertResult::Full`.
    ///     If the bucket is full but at least one node is disconnected and there is no pending
    ///     node, the new node is inserted as pending, yielding `InsertResult::Pending`.
    ///     Otherwise the bucket has free slots and the new node is added to the end of the
    ///     bucket as the most-recently connected node.
    ///
    ///   * `NodeStatus::Disconnected`: If the bucket is full, insertion fails with
    ///     `InsertResult::Full`. Otherwise the bucket has free slots and the new node
    ///     is inserted at the position preceding the first connected node,
    ///     i.e. as the most-recently disconnected node. If there are no connected nodes,
    ///     the new node is added as the last element of the bucket.
    ///
    pub fn insert(&mut self, node: Node<TKey, TVal>, status: NodeStatus) -> InsertResult<TKey> {
        let debug_node = node.clone(); // TODO: only for debugging. Should removed at some point.

        let result = if node.weight > 0 {
            self.weighted.insert(node, status)
        } else {
            self.swamp.insert(node, status)
        };

        log::debug!(
            "Bucket: inserting node {} weight {} -> {}",
            bs58::encode(debug_node.key.as_ref()).into_string(),
            debug_node.weight,
            result
        );

        result
    }

    fn is_full(&self, weighted: bool) -> bool {
        if weighted {
            self.weighted.is_full()
        } else {
            self.swamp.is_full()
        }
    }

    fn pending_active(&self, weighted: bool) -> bool {
        if weighted {
            self.weighted.pending_active()
        } else {
            self.swamp.pending_active()
        }
    }

    pub fn num_entries(&self) -> usize {
        self.swamp.num_entries() + self.weighted.num_entries()
    }

    pub fn status(&self, key: &TKey) -> Option<NodeStatus> {
        self.weighted.status(key).or(self.swamp.status(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::PeerId;
    use quickcheck::*;
    use rand::Rng;
    use std::collections::VecDeque;
    use std::time::Instant;

    impl Arbitrary for KBucket<Key<PeerId>, ()> {
        fn arbitrary<G: Gen>(g: &mut G) -> KBucket<Key<PeerId>, ()> {
            let timeout = Duration::from_secs(g.gen_range(1, g.size() as u64));
            let mut bucket = KBucket::<Key<PeerId>, ()>::new(timeout);
            let num_nodes = g.gen_range(1, K_VALUE.get() + 1);
            for _ in 0..num_nodes {
                let key = Key::new(PeerId::random());
                let node = Node {
                    key: key.clone(),
                    value: (),
                    weight: 0,
                };
                let status = NodeStatus::arbitrary(g);
                match bucket.insert(node, status) {
                    InsertResult::Inserted => {}
                    _ => panic!(),
                }
            }
            bucket
        }
    }

    impl Arbitrary for NodeStatus {
        fn arbitrary<G: Gen>(g: &mut G) -> NodeStatus {
            if g.gen() {
                NodeStatus::Connected
            } else {
                NodeStatus::Disconnected
            }
        }
    }

    impl Arbitrary for Position {
        fn arbitrary<G: Gen>(g: &mut G) -> Position {
            Position(g.gen_range(0, K_VALUE.get()))
        }
    }

    // Fill a bucket with random nodes with the given status.
    fn fill_bucket(bucket: &mut KBucket<Key<PeerId>, ()>, status: NodeStatus) {
        let num_entries_start = bucket.num_entries();
        for i in 0..K_VALUE.get() - num_entries_start {
            let key = Key::new(PeerId::random());
            let node = Node {
                key,
                value: (),
                weight: 0,
            };
            assert_eq!(InsertResult::Inserted, bucket.insert(node, status));
            assert_eq!(bucket.num_entries(), num_entries_start + i + 1);
        }
    }

    #[test]
    fn ordering() {
        fn prop(status: Vec<NodeStatus>) -> bool {
            let mut bucket = KBucket::<Key<PeerId>, ()>::new(Duration::from_secs(1));

            // The expected lists of connected and disconnected nodes.
            let mut connected = VecDeque::new();
            let mut disconnected = VecDeque::new();

            // Fill the bucket, thereby populating the expected lists in insertion order.
            for status in status {
                let key = Key::new(PeerId::random());
                let node = Node {
                    key: key.clone(),
                    value: (),
                    weight: 0,
                };
                let full = bucket.num_entries() == K_VALUE.get();
                match bucket.insert(node, status) {
                    InsertResult::Inserted => {
                        let vec = match status {
                            NodeStatus::Connected => &mut connected,
                            NodeStatus::Disconnected => &mut disconnected,
                        };
                        if full {
                            vec.pop_front();
                        }
                        vec.push_back((status, key.clone()));
                    }
                    _ => {}
                }
            }

            // Get all nodes from the bucket, together with their status.
            let mut nodes = bucket
                .iter()
                .map(|(n, s)| (s, n.key.clone()))
                .collect::<Vec<_>>();

            // Split the list of nodes at the first connected node.
            let first_connected_pos = nodes.iter().position(|(s, _)| *s == NodeStatus::Connected);
            let tail = first_connected_pos.map_or(Vec::new(), |p| nodes.split_off(p));

            // All nodes before the first connected node must be disconnected and
            // in insertion order. Similarly, all remaining nodes must be connected
            // and in insertion order.
            nodes == Vec::from(disconnected) && tail == Vec::from(connected)
        }

        quickcheck(prop as fn(_) -> _);
    }

    #[test]
    fn full_bucket() {
        let mut bucket = KBucket::<Key<PeerId>, ()>::new(Duration::from_secs(1));

        // Fill the bucket with disconnected nodes.
        fill_bucket(&mut bucket, NodeStatus::Disconnected);

        // Trying to insert another disconnected node fails.
        let key = Key::new(PeerId::random());
        let node = Node {
            key,
            value: (),
            weight: 0,
        };
        match bucket.insert(node, NodeStatus::Disconnected) {
            InsertResult::Full => {}
            x => panic!("Expected Full, got {:?}", x),
        }

        // One-by-one fill the bucket with connected nodes, replacing the disconnected ones.
        for _i in 0..K_VALUE.get() {
            let (first, first_status) = bucket.iter().next().unwrap();
            let first_disconnected = first.clone();
            assert_eq!(first_status, NodeStatus::Disconnected);

            // Add a connected node, which is expected to be pending, scheduled to
            // replace the first (i.e. least-recently connected) node.
            let key = Key::new(PeerId::random());
            let node = Node {
                key: key.clone(),
                value: (),
                weight: 0,
            };
            match bucket.insert(node.clone(), NodeStatus::Connected) {
                InsertResult::Pending { disconnected } => {
                    assert_eq!(disconnected, first_disconnected.key)
                }
                x => panic!("Expected Pending, got {:?}", x),
            }

            // Trying to insert another connected node fails.
            match bucket.insert(node.clone(), NodeStatus::Connected) {
                InsertResult::Full => {}
                x => panic!("Expected Full, got {:?}", x),
            }

            assert!(!bucket.pending().is_empty());

            // Apply the pending node.
            let pending = bucket.pending_mut(&key).expect("No pending node.");
            pending.set_ready_at(Instant::now() - Duration::from_secs(1));
            let result = bucket.apply_pending();
            assert_eq!(
                result,
                vec![AppliedPending {
                    inserted: node.clone(),
                    evicted: Some(first_disconnected)
                }]
            );
            assert_eq!(Some((&node, NodeStatus::Connected)), bucket.iter().last());
            assert!(bucket.pending().is_empty());
            /* assert_eq!(Some(K_VALUE.get() - (i + 1)), bucket.first_connected_pos); */
        }

        assert!(bucket.pending().is_empty());
        assert_eq!(K_VALUE.get(), bucket.num_entries());

        // Trying to insert another connected node fails.
        let key = Key::new(PeerId::random());
        let node = Node {
            key,
            value: (),
            weight: 0,
        };
        match bucket.insert(node, NodeStatus::Connected) {
            InsertResult::Full => {}
            x => panic!("{:?}", x),
        }
    }

    #[test]
    fn full_bucket_discard_pending() {
        let mut bucket = KBucket::<Key<PeerId>, ()>::new(Duration::from_secs(1));
        fill_bucket(&mut bucket, NodeStatus::Disconnected);
        let (first, _) = bucket.iter().next().unwrap();
        let first_disconnected = first.clone();

        // Add a connected pending node.
        let key = Key::new(PeerId::random());
        let node = Node {
            key: key.clone(),
            value: (),
            weight: 0,
        };
        if let InsertResult::Pending { disconnected } = bucket.insert(node, NodeStatus::Connected) {
            assert_eq!(&disconnected, &first_disconnected.key);
        } else {
            panic!()
        }
        assert!(!bucket.pending().is_empty());

        // Update the status of the first disconnected node to be connected.
        bucket.update(&first_disconnected.key, NodeStatus::Connected);

        // The pending node has been discarded.
        assert!(bucket.pending().is_empty());
        assert!(bucket.iter().all(|(n, _)| &n.key != &key));

        // The initially disconnected node is now the most-recently connected.
        assert_eq!(
            Some((&first_disconnected, NodeStatus::Connected)),
            bucket.iter().last()
        );

        let num_connected = bucket
            .iter()
            .filter(|(_, s)| *s == NodeStatus::Connected)
            .count();
        let num_disconnected = bucket
            .iter()
            .filter(|(_, s)| *s == NodeStatus::Disconnected)
            .count();
        let position = bucket
            .iter()
            .position(|(n, _)| n.key.as_ref() == first_disconnected.key.as_ref());

        assert_eq!(1, num_connected);
        assert_eq!(K_VALUE.get() - 1, num_disconnected);
        assert_eq!(position, Some(num_disconnected));
    }

    #[test]
    fn bucket_update() {
        fn prop(mut bucket: KBucket<Key<PeerId>, ()>, pos: usize, status: NodeStatus) -> bool {
            let mut nodes = bucket
                .iter()
                .map(|(n, s)| (n.clone(), s))
                .collect::<Vec<_>>();

            // Record the (ordered) list of status of all nodes in the bucket.
            let mut expected = nodes
                .iter()
                .map(|(n, s)| (n.key.clone(), *s))
                .collect::<Vec<_>>();

            // Capture position and key of the random node to update.
            let pos = pos % bucket.num_entries();
            let key = expected.iter().nth(pos).unwrap().0.clone();

            // Update the node in the bucket.
            bucket.update(&key, status);

            nodes.remove(pos);
            // Check that the bucket now contains the node with the new status,
            // preserving the status and relative order of all other nodes.
            let expected_pos = match status {
                NodeStatus::Connected => nodes.len(),
                NodeStatus::Disconnected => nodes
                    .iter()
                    .filter(|(_, s)| *s == NodeStatus::Disconnected)
                    .count(),
            };
            expected.remove(pos);
            expected.insert(expected_pos, (key.clone(), status));

            let actual = bucket
                .iter()
                .map(|(n, s)| (n.key.clone(), s))
                .collect::<Vec<_>>();

            assert_eq!(expected, actual, "pos was {}", pos);
            expected == actual
        }

        quickcheck(prop as fn(_, _, _) -> _);
    }
}
