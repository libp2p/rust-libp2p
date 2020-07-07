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

use crate::kbucket::{AppliedPending, InsertResult, KeyBytes, Node, NodeStatus, PendingNode, SubBucket, Position};
use crate::K_VALUE;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Swamp<TKey, TVal> {
    bucket: SubBucket<Node<TKey, TVal>>,
    pending: Option<PendingNode<TKey, TVal>>,
    pending_timeout: Duration,
}

impl<TKey, TVal> Swamp<TKey, TVal>
where
    TKey: Clone + AsRef<KeyBytes>,
    TVal: Clone,
{
    pub fn new(pending_timeout: Duration) -> Self {
        Self {
            bucket: SubBucket::new(K_VALUE.get()),
            pending: None,
            pending_timeout,
        }
    }

    pub fn set_pending(&mut self, node: PendingNode<TKey, TVal>) {
        self.pending = Some(node)
    }

    pub fn remove_pending(&mut self, key: &TKey) -> Option<PendingNode<TKey, TVal>> {
        let p = self.pending.as_ref()?;
        if p.node.key.as_ref() == key.as_ref() {
            self.drop_pending()
        } else {
            None
        }
    }

    pub fn drop_pending(&mut self) -> Option<PendingNode<TKey, TVal>> {
        self.pending.take()
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

    pub fn insert(&mut self, node: Node<TKey, TVal>, status: NodeStatus) -> InsertResult<TKey> {
        match status {
            NodeStatus::Connected => {
                if self.is_full() {
                    // TODO: use pending_active and call apply_pending?
                    if self.bucket.all_nodes_connected() || self.pending_exists() {
                        return InsertResult::Full;
                    } else {
                        self.set_pending(PendingNode {
                            node,
                            status: NodeStatus::Connected,
                            replace: Instant::now() + self.pending_timeout,
                        });
                        let least_recent = self
                            .bucket
                            .least_recently_connected()
                            .expect("bucket MUST be full here");
                        return InsertResult::Pending {
                            // Schedule a dial-up to check if the node is reachable
                            // NOTE: nodes[0] is disconnected (see all_nodes_connected check above)
                            //  and the least recently connected
                            disconnected: least_recent.key.clone(),
                        };
                    }
                }
                self.bucket.append_connected_node(node);
                InsertResult::Inserted
            }
            NodeStatus::Disconnected => {
                if self.is_full() {
                    return InsertResult::Full;
                }
                self.bucket.insert_disconnected_node(node);
                InsertResult::Inserted
            }
        }
    }

    pub fn apply_pending(&mut self) -> Option<AppliedPending<TKey, TVal>> {
        if !self.pending_ready() {
            return None;
        }

        self.pending.take().map(|PendingNode { node, status, .. }| {
            let evicted = if self.is_full() {
                self.bucket.pop_node()
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

    pub fn update(&mut self, key: &TKey, new_status: NodeStatus) -> bool {
        // Remove the node from its current position and then reinsert it
        // with the desired status, which puts it at the end of either the
        // prefix list of disconnected nodes or the suffix list of connected
        // nodes (i.e. most-recently disconnected or most-recently connected,
        // respectively).
        if let Some(pos) = self
            .bucket
            .position(|node| node.key.as_ref() == key.as_ref())
        {
            // Remove the node from its current position.
            let node = self
                .bucket
                .evict_node(pos)
                .expect("position MUST have been correct");
            // If the least-recently connected node re-establishes its
            // connected status, drop the pending node.
            if self.bucket.is_least_recently_connected(pos) && new_status == NodeStatus::Connected {
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

    pub fn iter(&self) -> impl Iterator<Item = (&Node<TKey, TVal>, NodeStatus)> {
        self.bucket.iter()
    }

    pub fn num_entries(&self) -> usize {
        self.bucket.nodes.len()
    }

    pub fn is_full(&self) -> bool {
        self.bucket.is_full()
    }

    pub fn status(&self, key: &TKey) -> Option<NodeStatus> {
        self.bucket
            .position(|node| node.key.as_ref() == key.as_ref())
            .map(|p| self.bucket.status(p))
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
        if let Some(position) = self
            .bucket
            .position(|node| node.key.as_ref() == key.as_ref())
        {
            self.bucket.get_mut(position)
        } else {
            None
        }
    }

    pub fn remove(&mut self, key: &TKey) ->  Option<(Node<TKey, TVal>, NodeStatus, Position)> {
        let pos = self.bucket.position(|n| n.key.as_ref() == key.as_ref())?;
        let status = self.bucket.status(pos.clone());
        let node = self.bucket.evict_node(pos.clone())?;

        Some((node, status, pos))
    }
}
