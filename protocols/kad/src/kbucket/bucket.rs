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
use futures::StreamExt;

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

enum ChangePosition {
    AddDisconnected,
    // num_entries – number of nodes in a bucket BEFORE appending
    AppendConnected { num_entries: usize },
    RemoveConnected,
    RemoveDisconnected,
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
        self.exists_active_pending(true) || self.exists_active_pending(false)
    }

    /// Returns a reference to the pending node of the bucket, if there is any.
    fn pending(&self) -> Option<&PendingNode<TKey, TVal>> {
        // self.swamp.as_ref()
        unimplemented!("pending")
    }

    /// Returns a mutable reference to the pending node of the bucket, if there is any.
    pub fn pending_mut(&mut self) -> Option<&mut PendingNode<TKey, TVal>> {
        // self.pending.as_mut()
        unimplemented!("pending_mut")
    }

    /// Returns a reference to the pending node of the bucket, if there is any
    /// with a matching key.
    pub fn as_pending(&self, key: &TKey) -> Option<&PendingNode<TKey, TVal>> {
        self.swamp_pending
            .iter()
            .chain(self.weighted_pending.iter())
            .find(|p| p.node.key.as_ref() == key.as_ref())
        // self.pending().filter(|p| p.node.key.as_ref() == key.as_ref())
    }

    /// Inserts the pending node into the bucket, if its timeout has elapsed,
    /// replacing the least-recently connected node.
    ///
    /// If a pending node has been inserted, its key is returned together with
    /// the node that was replaced. `None` indicates that the nodes in the
    /// bucket remained unchanged.
    pub fn apply_pending(&mut self) -> Option<AppliedPending<TKey, TVal>> {
        if !self.pending_ready() {
            return None;
        }

        self.swamp_pending
            .take()
            .map(|PendingNode { node, status, .. }| {
                let evicted = if self.is_full(node.weight > 0) {
                    Some(self.pop_node(node.weight))
                } else {
                    None
                };

                if let InsertResult::Inserted = self.insert(node.clone(), status) {
                    AppliedPending {
                        inserted: node,
                        evicted,
                    }
                } else {
                    unreachable!("Bucket is not full, we just evicted a node.")
                }
            })
    }

    /// Updates the status of the pending node, if any.
    pub fn update_pending(&mut self, status: NodeStatus) {
        if let Some(pending) = &mut self.swamp_pending {
            pending.status = status
        }
    }

    /// Returns an iterator over the nodes in the bucket, together with their status.
    pub fn iter(&self) -> impl Iterator<Item = (&Node<TKey, TVal>, NodeStatus)> {
        self.weighted
            .values()
            .map(|bucket| bucket.iter())
            .flatten()
            .chain(self.swamp.iter())
    }

    /// Updates the status of the node referred to by the given key, if it is
    /// in the bucket.
    pub fn update(&mut self, key: &TKey, new_status: NodeStatus) {
        // Remove the node from its current position and then reinsert it
        // with the desired status, which puts it at the end of either the
        // prefix list of disconnected nodes or the suffix list of connected
        // nodes (i.e. most-recently disconnected or most-recently connected,
        // respectively).
        if let Some(pos) = self.position(key) {
            // Remove the node from its current position.
            let node = self.evict_node(pos);
            // If the least-recently connected node re-establishes its
            // connected status, drop the pending node.
            if self.is_least_recently_connected(pos) && new_status == NodeStatus::Connected {
                self.remove_pending();
            }
            // Reinsert the node with the desired status.
            match self.insert(node, new_status) {
                InsertResult::Inserted => {}
                _ => unreachable!("The node is removed before being (re)inserted."),
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
    pub fn insert(&mut self, node: Node<TKey, TVal>, status: NodeStatus) -> InsertResult<TKey> {}

    fn pending_ready(&self, weighted: bool) -> bool {
        if weighted {
            self.weighted.pending_ready()
        } else {
            self.swamp.pending_ready()
        }
    }

    fn is_full(&self, weighted: bool) -> bool {
        if weighted {
            self.weighted.is_full()
        } else {
            self.swamp.is_full()
        }
    }

    fn exists_active_pending(&self, weighted: bool) -> bool {
        if weighted {
            self.weighted.pending_active()
        } else {
            self.swamp.exists_active_pending() // TODO: check if replace has passed?
        }
    }

    fn set_pending(&mut self, node: PendingNode<TKey, TVal>) {
        if node.node.weight == 0 {
            self.swamp.set_pending(node)
        } else {
            self.weighted.set_pending(node)
        }
    }

    fn remove_pending(&mut self) {
        if node.node.weight == 0 {
            self.swamp.remove_pending()
        } else {
            self.weighted.remove_pending()
        }
    }

    fn all_nodes_connected(&self, weight: u32) -> bool {
        if weight == 0 {
            self.swamp.all_nodes_connected()
        } else {
            self.weighted.all_nodes_connected(weight)
        }
    }

    fn append_connected_node(&mut self, node: Node<TKey, TVal>) {
        // `num_entries` MUST be calculated BEFORE insertion
        self.change_connected_pos(ChangePosition::AppendConnected {
            num_entries: self.num_entries(),
        });
        self.nodes.push(node);
    }

    fn insert_disconnected_node(&mut self, node: Node<TKey, TVal>) {
        let current_position = self.first_connected_pos;
        self.change_connected_pos(ChangePosition::AddDisconnected);
        match current_position {
            Some(p) => self.nodes.insert(p, node), // Insert disconnected node just before the first connected node
            None => self.nodes.push(node),         // Or simply append disconnected node
        }
    }

    fn evict_node(&mut self, position: Position) -> Node<TKey, TVal> {
        match self.status(position) {
            NodeStatus::Connected => self.change_connected_pos(ChangePosition::RemoveConnected),
            NodeStatus::Disconnected => {
                self.change_connected_pos(ChangePosition::RemoveDisconnected)
            }
        }
        self.nodes.remove(position.0)
    }

    fn pop_node(&mut self, weight: u32) -> Node<TKey, TVal> {
        self.evict_node(Position(0))
    }

    fn change_connected_pos(&mut self, action: ChangePosition) {
        match action {
            ChangePosition::AddDisconnected => {
                // New disconnected node added => position of the first connected node moved by 1
                self.first_connected_pos = self.first_connected_pos.map(|p| p + 1)
            }
            ChangePosition::AppendConnected { num_entries } => {
                // If there were no previously connected nodes – set mark to the given one (usually the last one)
                // Otherwise – keep it the same
                self.first_connected_pos = self.first_connected_pos.or(Some(num_entries));
            }
            ChangePosition::RemoveConnected => {
                if self.num_connected() == 1 {
                    // If it was the last connected node
                    self.first_connected_pos = None // Then mark there is no connected nodes left
                }
                // Otherwise – keep mark the same
            }
            ChangePosition::RemoveDisconnected => {
                // If there are connected nodes – lower mark
                // Otherwise – keep it None
                self.first_connected_pos = self
                    .first_connected_pos
                    .map(|p| p.checked_sub(1).unwrap_or(0))
            }
        }
    }

    fn is_least_recently_connected(&self, pos: Position) -> bool {
        pos.0 == 0
    }

    /// Returns the status of the node at the given position.
    pub fn status(&self, pos: Position) -> NodeStatus {
        if self.first_connected_pos.map_or(false, |i| pos.0 >= i) {
            NodeStatus::Connected
        } else {
            NodeStatus::Disconnected
        }
    }

    /// Checks whether the given position refers to a connected node.
    pub fn is_connected(&self, pos: Position) -> bool {
        self.status(pos) == NodeStatus::Connected
    }

    /// Gets the number of entries currently in the bucket.
    pub fn num_entries(&self) -> usize {
        self.nodes.len()
    }

    /// Gets the number of entries in the bucket that are considered connected.
    pub fn num_connected(&self) -> usize {
        self.first_connected_pos
            .map_or(0, |i| self.num_entries() - i)
    }

    /// Gets the number of entries in the bucket that are considered disconnected.
    pub fn num_disconnected(&self) -> usize {
        self.num_entries() - self.num_connected()
    }

    /// Gets the position of an node in the bucket.
    pub fn position(&self, key: &TKey) -> Option<Position> {
        self.nodes
            .iter()
            .position(|p| p.key.as_ref() == key.as_ref())
            .map(Position)
    }

    /// Gets a mutable reference to the node identified by the given key.
    ///
    /// Returns `None` if the given key does not refer to a node in the
    /// bucket.
    pub fn get_mut(&mut self, key: &TKey) -> Option<&mut Node<TKey, TVal>> {
        self.nodes
            .iter_mut()
            .find(move |p| p.key.as_ref() == key.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::PeerId;
    use quickcheck::*;
    use rand::Rng;
    use std::collections::VecDeque;

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
            let node = Node { key, value: () };
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
            assert_eq!(bucket.first_connected_pos, first_connected_pos);
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
        let node = Node { key, value: () };
        match bucket.insert(node, NodeStatus::Disconnected) {
            InsertResult::Full => {}
            x => panic!("{:?}", x),
        }

        // One-by-one fill the bucket with connected nodes, replacing the disconnected ones.
        for i in 0..K_VALUE.get() {
            let (first, first_status) = bucket.iter().next().unwrap();
            let first_disconnected = first.clone();
            assert_eq!(first_status, NodeStatus::Disconnected);

            // Add a connected node, which is expected to be pending, scheduled to
            // replace the first (i.e. least-recently connected) node.
            let key = Key::new(PeerId::random());
            let node = Node {
                key: key.clone(),
                value: (),
            };
            match bucket.insert(node.clone(), NodeStatus::Connected) {
                InsertResult::Pending { disconnected } => {
                    assert_eq!(disconnected, first_disconnected.key)
                }
                x => panic!("{:?}", x),
            }

            // Trying to insert another connected node fails.
            match bucket.insert(node.clone(), NodeStatus::Connected) {
                InsertResult::Full => {}
                x => panic!("{:?}", x),
            }

            assert!(bucket.pending().is_some());

            // Apply the pending node.
            let pending = bucket.pending_mut().expect("No pending node.");
            pending.set_ready_at(Instant::now() - Duration::from_secs(1));
            let result = bucket.apply_pending();
            assert_eq!(
                result,
                Some(AppliedPending {
                    inserted: node.clone(),
                    evicted: Some(first_disconnected)
                })
            );
            assert_eq!(Some((&node, NodeStatus::Connected)), bucket.iter().last());
            assert!(bucket.pending().is_none());
            assert_eq!(Some(K_VALUE.get() - (i + 1)), bucket.first_connected_pos);
        }

        assert!(bucket.pending().is_none());
        assert_eq!(K_VALUE.get(), bucket.num_entries());

        // Trying to insert another connected node fails.
        let key = Key::new(PeerId::random());
        let node = Node { key, value: () };
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
        };
        if let InsertResult::Pending { disconnected } = bucket.insert(node, NodeStatus::Connected) {
            assert_eq!(&disconnected, &first_disconnected.key);
        } else {
            panic!()
        }
        assert!(bucket.pending().is_some());

        // Update the status of the first disconnected node to be connected.
        bucket.update(&first_disconnected.key, NodeStatus::Connected);

        // The pending node has been discarded.
        assert!(bucket.pending().is_none());
        assert!(bucket.iter().all(|(n, _)| &n.key != &key));

        // The initially disconnected node is now the most-recently connected.
        assert_eq!(
            Some((&first_disconnected, NodeStatus::Connected)),
            bucket.iter().last()
        );
        assert_eq!(
            bucket.position(&first_disconnected.key).map(|p| p.0),
            bucket.first_connected_pos
        );
        assert_eq!(1, bucket.num_connected());
        assert_eq!(K_VALUE.get() - 1, bucket.num_disconnected());
    }

    #[test]
    fn bucket_update() {
        fn prop(mut bucket: KBucket<Key<PeerId>, ()>, pos: Position, status: NodeStatus) -> bool {
            let num_nodes = bucket.num_entries();

            // Capture position and key of the random node to update.
            let pos = pos.0 % num_nodes;
            let key = bucket.nodes[pos].key.clone();

            // Record the (ordered) list of status of all nodes in the bucket.
            let mut expected = bucket
                .iter()
                .map(|(n, s)| (n.key.clone(), s))
                .collect::<Vec<_>>();

            // Update the node in the bucket.
            bucket.update(&key, status);

            // Check that the bucket now contains the node with the new status,
            // preserving the status and relative order of all other nodes.
            let expected_pos = match status {
                NodeStatus::Connected => num_nodes - 1,
                NodeStatus::Disconnected => bucket.first_connected_pos.unwrap_or(num_nodes) - 1,
            };
            expected.remove(pos);
            expected.insert(expected_pos, (key.clone(), status));
            let actual = bucket
                .iter()
                .map(|(n, s)| (n.key.clone(), s))
                .collect::<Vec<_>>();
            expected == actual
        }

        quickcheck(prop as fn(_, _, _) -> _);
    }
}
