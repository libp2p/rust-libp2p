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

use std::time::Instant;
use log::debug;

enum ChangePosition {
    AddDisconnected,
    // num_entries – number of nodes in a bucket BEFORE appending
    AppendConnected { num_entries: usize },
    RemoveConnected,
    RemoveDisconnected,
}

/// The status of a node in a bucket.
///
/// The status of a node in a bucket together with the time of the
/// last status change determines the position of the node in a
/// bucket.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum NodeStatus {
    /// The node is considered connected.
    Connected,
    /// The node is considered disconnected.
    Disconnected,
}

/// A `PendingNode` is a `Node` that is pending insertion into a `KBucket`.
#[derive(Debug, Clone)]
pub struct PendingNode<TKey, TVal> {
    /// The pending node to insert.
    pub node: Node<TKey, TVal>,

    /// The status of the pending node.
    pub status: NodeStatus,

    /// The instant at which the pending node is eligible for insertion into a bucket.
    pub replace: Instant,
}

impl<TKey, TVal> PendingNode<TKey, TVal> {
    pub fn key(&self) -> &TKey {
        &self.node.key
    }

    pub fn status(&self) -> NodeStatus {
        self.status
    }

    pub fn value_mut(&mut self) -> &mut TVal {
        &mut self.node.value
    }

    pub fn is_ready(&self) -> bool {
        Instant::now() >= self.replace
    }

    pub fn set_ready_at(&mut self, t: Instant) {
        self.replace = t;
    }

    pub fn into_node(self) -> Node<TKey, TVal> {
        self.node
    }
}

/// A `Node` in a bucket, representing a peer participating
/// in the Kademlia DHT together with an associated value (e.g. contact
/// information).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node<TKey, TVal> {
    /// The key of the node, identifying the peer.
    pub key: TKey,
    /// The associated value.
    pub value: TVal,
    pub weight: u32,
}

/// The position of a node in a `KBucket`, i.e. a non-negative integer
/// in the range `[0, K_VALUE)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position(pub usize);

#[derive(Debug, Clone)]
pub struct SubBucket<Node> {
    pub nodes: Vec<Node>,
    pub first_connected_pos: Option<usize>,
    pub capacity: usize,
}

impl<Node> SubBucket<Node> {
    pub fn new(capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(capacity + 1),
            first_connected_pos: None,
            capacity,
        }
    }

    pub fn status(&self, pos: Position) -> NodeStatus {
        if self.first_connected_pos.map_or(false, |i| pos.0 >= i) {
            NodeStatus::Connected
        } else {
            NodeStatus::Disconnected
        }
    }

    /// Returns an iterator over the nodes in the bucket, together with their status.
    pub fn iter(&self) -> impl Iterator<Item = (&Node, NodeStatus)> {
        self.nodes
            .iter()
            .enumerate()
            .map(move |(p, n)| (n, self.status(Position(p))))
    }

    pub fn is_full(&self) -> bool {
        self.nodes.len() >= self.capacity
    }

    pub fn all_nodes_connected(&self) -> bool {
        self.first_connected_pos == Some(0)
    }

    pub fn append_connected_node(&mut self, node: Node) {
        // `num_entries` MUST be calculated BEFORE insertion
        self.change_connected_pos(ChangePosition::AppendConnected {
            num_entries: self.num_entries(),
        });
        self.nodes.push(node);
    }

    pub fn insert_disconnected_node(&mut self, node: Node) {
        let current_position = self.first_connected_pos;
        self.change_connected_pos(ChangePosition::AddDisconnected);
        match current_position {
            Some(p) => self.nodes.insert(p, node), // Insert disconnected node just before the first connected node
            None => self.nodes.push(node),         // Or simply append disconnected node
        }
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

    pub fn evict_node(&mut self, position: Position) -> Option<Node> {
        match self.status(position) {
            NodeStatus::Connected => self.change_connected_pos(ChangePosition::RemoveConnected),
            NodeStatus::Disconnected => {
                self.change_connected_pos(ChangePosition::RemoveDisconnected)
            }
        }
        if position.0 >= self.nodes.len() {
            debug!(
                "WARNING: tried to evict node at {} while there's only {} nodes",
                position.0,
                self.nodes.len()
            );
            None
        } else {
            Some(self.nodes.remove(position.0))
        }
    }

    pub fn pop_node(&mut self) -> Option<Node> {
        self.evict_node(Position(0))
    }

    pub fn least_recently_connected(&self) -> Option<&Node> {
        self.nodes.get(0)
    }

    pub fn is_least_recently_connected(&self, pos: Position) -> bool {
        pos.0 == 0
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
    pub fn position<P>(&self, pred: P) -> Option<Position>
    where
        P: Fn(&Node) -> bool,
    {
        self.nodes.iter().position(pred).map(Position)
    }

    pub fn get_mut(&mut self, position: Position) -> Option<&mut Node> {
        self.nodes.get_mut(position.0)
    }
}
