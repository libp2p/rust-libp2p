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
    AppliedPending, InsertResult, KeyBytes, Node, NodeStatus, PendingNode, Position, SubBucket,
};
use crate::W_VALUE;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use log::trace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightedNode<TKey, TVal> {
    pub inner: Node<TKey, TVal>,
    // TODO: refresh last_contact_time
    // TODO: move to bucket
    pub last_contact_time: Option<Instant>,
}

impl<TKey, TVal> WeightedNode<TKey, TVal> {
    fn update(mut self, status: NodeStatus) -> Self {
        let new_time = if status == NodeStatus::Connected {
            Some(Instant::now())
        } else {
            None
        };
        self.last_contact_time = new_time;

        self
    }
}

impl<TKey, TVal> Into<Node<TKey, TVal>> for WeightedNode<TKey, TVal> {
    fn into(self) -> Node<TKey, TVal> {
        self.inner
    }
}

impl<TKey, TVal> From<Node<TKey, TVal>> for WeightedNode<TKey, TVal> {
    fn from(node: Node<TKey, TVal>) -> Self {
        Self {
            inner: node,
            last_contact_time: None,
        }
    }
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

impl Into<Position> for WeightedPosition {
    fn into(self) -> Position {
        Position(self.position)
    }
}

#[derive(Debug, Clone)]
pub struct Weighted<TKey, TVal> {
    map: HashMap<u32, SubBucket<WeightedNode<TKey, TVal>>>,
    pending: Option<PendingNode<TKey, TVal>>,
    capacity: usize,
    pending_timeout: Duration,
}

impl<TKey, TVal> Weighted<TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
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
            .map_or(false, |bucket| bucket.all_nodes_connected())
    }

    pub fn set_pending(&mut self, node: PendingNode<TKey, TVal>) {
        self.pending = Some(node)
    }

    pub fn remove_pending(&mut self, key: &TKey) -> Option<PendingNode<TKey, TVal>> {
        // let matches = self.pending.map_or(false, |n| n.node.key.as_ref() == key.as_ref());
        // if matches {
        //     self.pending.take().map(|n| n.into())
        // } else {
        //     None
        // }

        let p = self.pending.as_ref()?;
        if p.node.key.as_ref() == key.as_ref() {
            self.drop_pending()
        } else {
            None
        }
    }

    // TODO: pending 1. Refactor?
    pub fn pending_ready(&self) -> bool {
        self.pending
            .as_ref()
            .map_or(false, |pending| pending.is_ready())
    }

    // TODO: pending 2. Refactor?
    pub fn pending_active(&self) -> bool {
        self.pending
            .as_ref()
            .map_or(false, |pending| !pending.is_ready())
    }

    // TODO: pending 3. Refactor?
    pub fn pending_exists(&self) -> bool {
        self.pending.is_some()
    }

    pub fn num_entries(&self) -> usize {
        self.map.values().map(|bucket| bucket.nodes.len()).sum()
    }

    pub fn is_full(&self) -> bool {
        self.num_entries() >= self.capacity
    }

    fn get_bucket_mut(&mut self, weight: u32) -> &mut SubBucket<WeightedNode<TKey, TVal>> {
        self.map
            .entry(weight)
            .or_insert(SubBucket::new(W_VALUE.get()))
    }

    fn append_connected_node(&mut self, node: WeightedNode<TKey, TVal>) {
        self.get_bucket_mut(node.inner.weight)
            .append_connected_node(node)
    }

    fn insert_disconnected_node(&mut self, node: WeightedNode<TKey, TVal>) {
        self.get_bucket_mut(node.inner.weight)
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
            .filter(|(weight, bucket)| **weight <= weight_bound && !bucket.nodes.is_empty())
            .min_by(|(weight_a, bucket_a), (weight_b, bucket_b)| {
                // First compare by weight, then compare by recency
                Ord::cmp(weight_a, weight_b).then(Ord::cmp(
                    &bucket_a
                        .least_recently_connected()
                        .and_then(|n| n.last_contact_time),
                    &bucket_b
                        .least_recently_connected()
                        .and_then(|n| n.last_contact_time),
                ))
            })
            .and_then(|(_, bucket)| bucket.least_recently_connected())
    }

    fn evict_node(&mut self, position: WeightedPosition) -> Option<WeightedNode<TKey, TVal>> {
        let bucket = self.get_bucket_mut(position.weight);
        bucket.evict_node(position.into())
    }

    fn pop_node(&mut self, weight_bound: u32) -> Option<WeightedNode<TKey, TVal>> {
        if let Some(weight) = self.least_recent(weight_bound).map(|lr| lr.inner.weight) {
            let bucket = self.get_bucket_mut(weight);
            bucket.pop_node()
        } else {
            None // TODO: what it means if there's no least_recent node?
        }
    }

    fn position(&self, key: &TKey) -> Option<WeightedPosition> {
        self.map.iter().find_map(|(&weight, bucket)| {
            bucket
                .position(|node| node.inner.key.as_ref() == key.as_ref())
                .map(|pos| WeightedPosition::new(weight, pos))
        })
    }

    fn is_least_recently_connected(&self, node: &WeightedNode<TKey, TVal>) -> bool {
        let least_recent = self.least_recent(node.inner.weight);

        least_recent.map_or(false, |l_r| {
            l_r.inner.key.as_ref() == node.inner.key.as_ref()
        })
    }

    fn status_of(&self, position: WeightedPosition) -> Option<NodeStatus> {
        self.map
            .get(&position.weight)
            .map(|bucket| bucket.status(position.into()))
    }

    fn drop_pending(&mut self) -> Option<PendingNode<TKey, TVal>> {
        self.pending.take()
    }

    pub fn insert<Node: Into<WeightedNode<TKey, TVal>>>(
        &mut self,
        node: Node,
        status: NodeStatus,
    ) -> InsertResult<TKey> {
        let node = node.into().update(status);

        match status {
            NodeStatus::Connected => {
                if !self.is_full() {
                    // If there's free space in bucket, append the node
                    self.append_connected_node(node);
                    InsertResult::Inserted
                } else {
                    let min_key = self.min_key().expect("bucket MUST be full here");

                    // TODO: use pending_active and call apply_pending?
                    if min_key < node.inner.weight && !self.pending_exists() {
                        // If bucket is full, but there's a sub-bucket with lower weight, and no pending node
                        // then set `node` to be pending, and schedule a dial-up check for the least recent node
                        match self
                            .least_recent(node.inner.weight)
                            .map(|lr| lr.inner.key.clone())
                        {
                            Some(least_recent_key) => {
                                self.set_pending(PendingNode {
                                    node: node.inner,
                                    status,
                                    replace: Instant::now() + self.pending_timeout,
                                });
                                InsertResult::Pending {
                                    disconnected: least_recent_key,
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
            .and_then(|PendingNode { node, status, .. }| {
                let evicted = if self.is_full() {
                    self.pop_node(node.weight)
                } else {
                    None
                };

                if let InsertResult::Inserted = self.insert(node.clone(), status) {
                    Some(AppliedPending {
                        inserted: node,
                        evicted: evicted.map(|e| e.into()),
                    })
                } else {
                    // NOTE: this is different from Swamp. Here it is possible that insert will return Full
                    //       because we can only evict a node with weight <= pending.weight. So it is possible
                    //       that bucket ISN'T FULL, but insert returns InsertResult::Full
                    None
                }
            })
    }

    pub fn update(&mut self, key: &TKey, new_status: NodeStatus) -> bool {
        if let Some(pos) = self.position(key) {
            // Remove the node from its current position.
            let node = self
                .evict_node(pos)
                .expect("position MUST have been correct");
            // If the least-recently connected node re-establishes its
            // connected status, drop the pending node.
            if self.is_least_recently_connected(&node) && new_status == NodeStatus::Connected {
                self.drop_pending();
            }
            // Reinsert the node with the desired status.
            match self.insert(node, new_status) {
                InsertResult::Inserted => {}
                _ => unreachable!("The node is removed before being (re)inserted."),
            }

            true
        } else {
            false
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Node<TKey, TVal>, NodeStatus)> + '_ {
        let mut keys = self.map.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        let map: &HashMap<u32, _> = &self.map;

        keys.into_iter()
            .map(move |w| {
                trace!("Weighted: iterating through {} weight", w);
                map.get(&w)
                    .into_iter()
                    .map(|bucket| bucket.iter().map(|(n, s)| (&n.inner, s)))
                    .flatten()
            })
            .flatten()
    }

    pub fn status(&self, key: &TKey) -> Option<NodeStatus> {
        self.position(key).and_then(|position| {
            self.status_of(position)
        })
    }

    pub fn update_pending(&mut self, key: &TKey, status: NodeStatus) -> bool {
        self.pending_mut(key).map_or(false, |pending| {
            pending.status = status;
            true
        })
    }

    pub fn pending(&self) -> Option<&PendingNode<TKey, TVal>> {
        self.pending.as_ref()
    }

    pub fn pending_mut(&mut self, key: &TKey) -> Option<&mut PendingNode<TKey, TVal>> {
        if let Some(pending) = self.pending.as_mut() {
            if pending.key().as_ref() == key.as_ref() {
                return Some(pending);
            }
        }

        return None;
    }

    pub fn pending_ref(&self, key: &TKey) -> Option<&PendingNode<TKey, TVal>> {
        if let Some(pending) = self.pending.as_ref() {
            if pending.key().as_ref() == key.as_ref() {
                return Some(pending);
            }
        }

        return None;
    }

    pub fn get_mut(&mut self, key: &TKey) -> Option<&mut Node<TKey, TVal>> {
        let position = self.position(key)?;
        self.get_bucket_mut(position.weight)
            .get_mut(position.into())
            .map(|n| &mut n.inner)
    }

    pub fn remove(&mut self, key: &TKey) ->  Option<(Node<TKey, TVal>, NodeStatus, Position)> {
        let pos = self.position(key).clone()?;
        // TODO: excess clone
        let status = self.status_of(pos.clone())?;
        let node = self.evict_node(pos.clone())?;

        Some((node.into(), status, pos.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kbucket::Key;
    use libp2p_core::PeerId;
    use quickcheck::*;

    #[test]
    fn simple_insert() {
        fn prop(weight_status: Vec<(u32, NodeStatus)>) -> bool {
            use NodeStatus::*;

            let mut bucket = Weighted::new(Duration::from_secs(100000));
            for (i, (weight, status)) in weight_status.into_iter().enumerate() {
                let key = Key::new(PeerId::random());
                let node = Node {
                    key: key.clone(),
                    value: (),
                    weight,
                };

                let result = bucket.insert(node, status);
                if i < W_VALUE.get() {
                    assert_eq!(result, InsertResult::Inserted, "position {}", i);
                } else {
                    match result {
                        InsertResult::Pending { .. } => {
                            assert_eq!(
                                status, Connected,
                                "Only Connected nodes could become pending"
                            );
                            assert!(bucket.pending.is_some());
                        }
                        InsertResult::Inserted => {
                            assert!(
                                false,
                                "There shouldn't be a place in the bucket. {} {:?}",
                                i, status
                            );
                        }
                        InsertResult::Full => {}
                    }
                }
            }

            true
        }

        quickcheck(prop as fn(_) -> _);
    }

    #[test]
    fn weight_priority() {
        fn prop(weight_status: Vec<(u32, NodeStatus)>) -> bool {
            let mut bucket = Weighted::new(Duration::from_secs(100000));

            let mut map: HashMap<u32, Vec<(Node<Key<PeerId>, ()>, NodeStatus)>> = HashMap::new();
            for (weight, status) in weight_status {
                let node: Node<Key<PeerId>, ()> = Node {
                    key: Key::new(PeerId::random()),
                    value: (),
                    weight,
                };

                match bucket.insert(node.clone(), status.clone()) {
                    InsertResult::Inserted => {
                        let nodes = map.entry(node.weight).or_insert(Vec::new());
                        let position = match status {
                            NodeStatus::Connected => nodes.len(),
                            NodeStatus::Disconnected => nodes
                                .iter()
                                .filter(|t| t.1 == NodeStatus::Disconnected)
                                .count(),
                        };

                        nodes.insert(position, (node, status))
                    }
                    _ => {}
                }
            }

            let mut expected = map.into_iter().collect::<Vec<_>>();
            expected.sort_by_key(|(k, _)| *k);
            let expected = expected
                .into_iter()
                .flat_map(|(_, ns)| ns)
                .map(|(n, s)| (n.key, n.weight, s))
                .collect::<Vec<_>>();

            let bucket_nodes = bucket
                .iter()
                .map(|(n, s)| (n.key.clone(), n.weight, s))
                .collect::<Vec<_>>();

            // Checking the same thing 3 times, why not? It's better to перебдеть.
            assert_eq!(expected.len(), bucket_nodes.len());
            for i in 0..expected.len() {
                assert_eq!(expected[i], bucket_nodes[i], "\n\nposition {}", i);
            }

            expected == bucket_nodes
        }

        quickcheck(prop as fn(_) -> _);
    }
}
