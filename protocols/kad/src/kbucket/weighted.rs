/*
 * Copyright 2019 Fluence Labs Limited
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
    AppliedPending, InsertResult, Node, NodeStatus, PendingNode, Position, SubBucket,
};
use crate::W_VALUE;
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedPendingNode<TKey, TVal> {
    /// The pending node to insert.
    pub node: WeightedNode<TKey, TVal>,

    /// The status of the pending node.
    pub status: NodeStatus,

    /// The instant at which the pending node is eligible for insertion into a bucket.
    pub replace: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedNode<TKey, TVal> {
    /// The key of the node, identifying the peer.
    pub key: TKey,
    /// The associated value.
    pub value: TVal,
    pub weight: u32,
    pub last_contact_time: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedPosition {
    pub weight: u32,
    pub position: usize,
}

impl WeightedPosition {
    pub fn new(weight: u32, position: Position) -> Self {
        Self {
            weight,
            position: position.0,
        }
    }
}

impl<TKey, TVal> Into<Node<TKey, TVal>> for WeightedNode<TKey, TVal> {
    fn into(self) -> Node<TKey, TVal> {
        Node {
            key: self.key,
            value: self.value,
            weight: self.weight,
        }
    }
}

pub struct Weighted<TKey, TVal> {
    map: HashMap<u32, SubBucket<WeightedNode<TKey, TVal>>>,
    pending: Option<WeightedPendingNode<TKey, TVal>>,
    capacity: usize,
    pending_timeout: Duration,
}

impl<TKey, TVal> Weighted<TKey, TVal> {
    pub fn new(pending_timeout: Duration) -> Self {
        Self {
            map: HashMap::new(),
            pending: None,
            capacity: W_VALUE.get(),
            pending_timeout,
        }
    }

    pub fn all_nodes_connected(&self, weight: u32) -> bool {
        self.map
            .get(&weight)
            .map_or_else(false, |bucket| bucket.all_nodes_connected())
    }

    pub fn pending_active(&self) -> bool {
        self.pending.is_some() // TODO: check replace timeout
    }

    pub fn set_pending(&mut self, node: WeightedPendingNode<TKey, TVal>) {
        self.pending = Some(node)
    }

    pub fn remove_pending(&mut self) {
        self.pending = None
    }

    pub fn pending_ready(&self) -> bool {
        self.pending
            .as_ref()
            .map_or(false, |pending| pending.replace <= Instant::now())
    }

    fn num_entries(&self) -> usize {
        self.map.values().map(|bucket| bucket.nodes.len()).sum()
    }

    pub fn is_full(&self) -> bool {
        self.num_entries() >= self.capacity
    }

    fn get_bucket_mut(&mut self, weight: u32) -> &mut SubBucket<WeightedNode<TKey, TVal>> {
        match self.map.entry(weight) {
            Entry::Occupied(mut e) => e.get_mut(),
            Entry::Vacant(e) => {
                let mut bucket = SubBucket::new();
                bucket.append_connected_node(node);
                bucket
            }
        }
    }

    fn append_connected_node(&mut self, node: WeightedNode<TKey, TVal>) {
        self.get_bucket_mut(node.weight).append_connected_node(node)
    }

    fn insert_disconnected_node(&mut self, node: WeightedNode<TKey, TVal>) {
        self.get_bucket_mut(node.weight)
            .insert_disconnected_node(node)
    }

    fn min_key(&self) -> Option<u32> {
        self.map.keys().min().cloned()
    }

    fn least_recent(&self, weight_bound: u32) -> Option<&WeightedNode<TKey, TVal>> {
        // Search through top nodes in each bucket (assuming they are sorted by contact time),
        // and then take their top element
        self.map
            .iter()
            .filter(|(&&key, _)| key <= weight_bound)
            .min_by(|((_, bucket_a), (_, bucket_b))| {
                Ord::cmp(
                    bucket_a
                        .least_recently_connected()
                        .map(|n| n.last_contact_time),
                    bucket_b
                        .least_recently_connected()
                        .map(|n| n.last_contact_time),
                )
            })
            .and_then(|(_, bucket)| bucket.least_recently_connected())
    }

    fn evict_node(&mut self, position: WeightedPosition) -> Option<WeightedNode<TKey, TVal>> {
        let mut bucket = self.get_bucket_mut(position.weight);
        bucket.evict_node(Position(position.position)) // Position.position.Position(position.Position)
    }

    fn pop_node(&mut self, weight_bound: u32) -> Option<WeightedNode<TKey, TVal>> {
        if let Some(least_recent) = self.least_recent(weight_bound) {
            let mut bucket = self.get_bucket_mut(least_recent.weight);
            bucket.pop_node()
        } else {
            None // TODO: what it means if there's no least_recent node?
        }
    }

    fn position(&self, key: TKey) -> Option<WeightedPosition> {
        self.map.iter().find_map(|(weight, bucket)| {
            bucket
                .position(key)
                .map(|pos| WeightedPosition(weight, pos))
        })
    }

    fn is_least_recently_connected(&self, node: &WeightedNode<TKey, TVal>) -> bool {
        let least_recent = self.least_recent(node.weight);

        least_recent.map_or(false, |l_r| l_r == node) // TODO: is it enough to compare references here?
    }

    pub fn insert(
        &mut self,
        node: WeightedNode<TKey, TVal>,
        status: NodeStatus,
    ) -> InsertResult<TKey> {
        match status {
            NodeStatus::Connected => {
                if !self.is_full() {
                    // If there's free space in bucket, append the node
                    self.append_connected_node(node);
                    InsertResult::Inserted
                } else {
                    let min_key = self.min_key().expect("bucket MUST be full here");

                    if min_key < node.weight && !self.pending_active() {
                        // If bucket is full, but there's a sub-bucket with lower weight, and no pending node
                        // then set `node` to be pending, and schedule a dial-up check for the least recent node
                        match self.least_recent(node.weight) {
                            Some(least_recent) => {
                                self.set_pending(WeightedPendingNode {
                                    node,
                                    status,
                                    replace: Instant::now() + self.pending_timeout,
                                });
                                InsertResult::Pending {
                                    disconnected: least_recent,
                                }
                            }
                            // There's no node to evict
                            None => InsertResult::Full,
                        }
                    } else {
                        InsertResult::Full
                    }
                }
            }
            NodeStatus::Disconnected if !self.is_full() => {
                self.insert_disconnected_node(node); // TODO: maybe schedule a dial-up to this node?
                InsertResult::Inserted
            }
            _ => InsertResult::Full,
        }
    }

    pub fn apply_pending(&mut self) -> Option<AppliedPending<TKey, TVal>> {
        if !self.pending_ready() {
            return None;
        }

        self.pending
            .take()
            .and_then(|WeightedPendingNode { node, status, .. }| {
                let evicted = if self.is_full() {
                    self.pop_node(node.weight)
                } else {
                    None
                };

                if let InsertResult::Inserted = self.insert(node.clone(), status) {
                    Some(AppliedPending {
                        inserted: node.into(),
                        evicted: evicted.into(),
                    })
                } else {
                    // NOTE: this is different from Swamp. Here it is possible that insert will return Full
                    //       because we can only evict a node with weight <= pending.weight. So it is possible
                    //       that bucket ISN'T FULL, but insert returns InsertResult::Full
                    None
                }
            })
    }

    pub fn update(&mut self, key: &TKey, new_status: NodeStatus) {
        // Remove the node from its current position and then reinsert it
        // with the desired status, which puts it at the end of either the
        // prefix list of disconnected nodes or the suffix list of connected
        // nodes (i.e. most-recently disconnected or most-recently connected,
        // respectively).
        if let Some(pos) = self.position(key) {
            // Remove the node from its current position.
            let node = self
                .evict_node(pos)
                .expect("position MUST have been correct");
            // If the least-recently connected node re-establishes its
            // connected status, drop the pending node.
            if self.is_least_recently_connected(&node) && new_status == NodeStatus::Connected {
                self.remove_pending();
            }
            // Reinsert the node with the desired status.
            match self.insert(node, new_status) {
                InsertResult::Inserted => {}
                _ => unreachable!("The node is removed before being (re)inserted."),
            }
        }
    }
}
