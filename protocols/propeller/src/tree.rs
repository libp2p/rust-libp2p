//! Dynamic propeller tree computation logic.
//!
//! This module implements the core tree topology algorithm inspired by Solana's Turbine protocol.
//! The tree is computed dynamically for each shred using deterministic seeded randomization
//! based on the publisher and shred ID, making the network resilient to targeted attacks.

use std::collections::HashMap;

use libp2p_identity::PeerId;
use rand::{
    distr::{weighted::WeightedIndex, Distribution},
    SeedableRng,
};
use rand_chacha::ChaChaRng;

use crate::{
    message::ShredHash,
    types::{PeerSetError, PropellerNode, TreeGenerationError},
};

/// A built propeller tree for a specific shred hash.
#[derive(Debug, Clone)]
pub struct PropellerTree {
    /// The shuffled peer IDs for this specific tree, ordered by weighted random selection.
    peers: Vec<PeerId>,
    /// Data plane fanout (number of children each node has).
    fanout: usize,
    /// This node's index in the shuffled peer IDs.
    /// None if the local peer is not in the tree (e.g., local peer is the publisher).
    local_index: Option<usize>,
}

/// Propeller tree manager that computes tree topology dynamically for each shred.
#[derive(Debug, Clone)]
pub(crate) struct PropellerTreeManager {
    /// All nodes in the cluster with their weights, sorted by (weight, peer_id) descending.
    nodes: Vec<PropellerNode>,
    /// This node's peer ID.
    local_peer_id: PeerId,
    /// Data plane fanout (number of children each node has).
    fanout: usize,
}

impl PropellerTreeManager {
    /// Create a new propeller tree manager.
    pub(crate) fn new(local_peer_id: PeerId, fanout: usize) -> Self {
        Self {
            nodes: Vec::new(),
            local_peer_id,
            fanout,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn clear(&mut self) {
        self.nodes.clear();
    }

    pub(crate) fn get_local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Update the cluster nodes with their weights.
    /// Nodes are sorted by (weight, peer_id) in descending order for deterministic behavior.
    pub(crate) fn update_nodes(
        &mut self,
        peer_weights: &HashMap<PeerId, u64>,
    ) -> Result<(), PeerSetError> {
        if !peer_weights.contains_key(&self.local_peer_id) {
            return Err(PeerSetError::LocalPeerNotInPeerWeights);
        }

        // Convert to PropellerNode and sort by weight descending, then by peer_id for determinism
        let mut nodes: Vec<PropellerNode> = peer_weights
            .iter()
            .map(|(&peer_id, &weight)| PropellerNode { peer_id, weight })
            .collect();

        nodes.sort_by(|a, b| {
            // Sort by weight descending, then by peer_id ascending for determinism
            b.weight
                .cmp(&a.weight)
                .then_with(|| a.peer_id.cmp(&b.peer_id))
        });

        self.nodes = nodes;
        Ok(())
    }

    /// Build a propeller tree for the given shred hash.
    ///
    /// The tree is built using weighted random selection from all nodes EXCEPT the publisher.
    /// Nodes with higher weights are more likely to be positioned earlier in the tree (closer
    /// to the root). The publisher sends shreds to the root node, which then propagates them
    /// through the tree. The publisher is not included in the tree structure itself.
    pub(crate) fn build_tree(
        &self,
        publisher: &PeerId,
        shred_hash: &ShredHash,
    ) -> Result<PropellerTree, TreeGenerationError> {
        let mut rng = ChaChaRng::from_seed(*shred_hash);

        // Find and exclude the publisher from the tree
        let publisher_node_index = self
            .nodes
            .iter()
            .position(|node| node.peer_id == *publisher)
            .ok_or(TreeGenerationError::PublisherNotFound {
                publisher: *publisher,
            })?;

        // Build list of indices excluding the publisher
        let mut indices_to_choose_from: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| i != publisher_node_index)
            .collect();
        let mut tree_indices = Vec::with_capacity(indices_to_choose_from.len());

        // Build tree using weighted random selection (publisher is excluded from tree)
        for _ in 0..indices_to_choose_from.len() {
            let weights: Vec<_> = indices_to_choose_from
                .iter()
                .map(|index| self.nodes[*index].weight)
                .collect();
            let weighted_index = WeightedIndex::new(weights).unwrap();
            let index_in_indices_to_choose_from = weighted_index.sample(&mut rng);
            let index = indices_to_choose_from.swap_remove(index_in_indices_to_choose_from);
            tree_indices.push(index);
        }

        assert_eq!(
            tree_indices.len(),
            self.nodes.len() - 1,
            "All non-publisher indices should be present"
        );
        assert!(
            {
                let mut a = tree_indices.clone();
                a.sort();
                a.dedup();
                a.len() == self.nodes.len() - 1
            },
            "All indices should be unique"
        );
        assert!(
            tree_indices.iter().all(|index| index < &self.nodes.len()),
            "All indices should be less than the number of nodes"
        );
        assert!(
            !tree_indices.contains(&publisher_node_index),
            "Publisher should not be in tree"
        );

        // Convert indices to actual peer IDs
        let peers: Vec<PeerId> = tree_indices
            .into_iter()
            .map(|index| self.nodes[index].peer_id)
            .collect();

        // If the local peer is the publisher, they won't be in the tree (excluded)
        // If the tree is empty (publisher is the only peer), local peer is also not in tree
        // Otherwise, find the local peer in the tree
        let local_index = if self.local_peer_id == *publisher || peers.is_empty() {
            None // Local peer is the publisher, not in tree
        } else {
            let idx = peers
                .iter()
                .position(|peer_id| *peer_id == self.local_peer_id)
                .ok_or(TreeGenerationError::LocalPeerNotInPeerWeights)?;
            assert_eq!(peers[idx], self.local_peer_id);
            Some(idx)
        };

        Ok(PropellerTree {
            peers,
            fanout: self.fanout,
            local_index,
        })
    }
}

impl PropellerTree {
    /// Get the children peer IDs for the local node in this tree.
    /// Returns an empty vector if the local node is not in the tree.
    pub(crate) fn get_children(&self) -> Vec<PeerId> {
        match self.local_index {
            Some(local_index) => get_tree_children(local_index, self.fanout, self.peers.len())
                .into_iter()
                .map(|index| self.peers[index])
                .collect(),
            None => Vec::new(),
        }
    }

    /// Get the parent peer ID for the local node in this tree.
    /// Returns None if the local node is not in the tree or is the root.
    pub(crate) fn get_parent(&self) -> Option<PeerId> {
        let local_index = self.local_index?;
        let parent = get_tree_parent(local_index, self.fanout);
        parent.map(|index| self.peers[index])
    }

    /// Get the root peer ID for this tree (the node at index 0).
    /// Returns None if the tree is empty.
    pub(crate) fn get_root(&self) -> Option<PeerId> {
        self.peers.first().copied()
    }

    /// Check if the local node is the root.
    /// Returns false if the local node is not in the tree or if the tree is empty.
    pub(crate) fn is_local_root(&self) -> bool {
        self.local_index == Some(0)
    }
}

/// Get the parent position for a node at the given position in a tree.
/// Returns the parent position in the shuffled node list, or None if this is the root.
fn get_tree_parent(position: usize, fanout: usize) -> Option<usize> {
    if position == 0 {
        None // Root has no parent
    } else {
        Some((position - 1) / fanout)
    }
}

/// Get the children indices for a node at the given position in a tree.
/// Returns the positions of children in the shuffled node list.
fn get_tree_children(position: usize, fanout: usize, total_nodes: usize) -> Vec<usize> {
    let mut children = Vec::new();

    // First child position = position * fanout + 1
    let first_child = position * fanout + 1;

    // Add up to `fanout` children
    for i in 0..fanout {
        let child_pos = first_child + i;
        if child_pos < total_nodes {
            children.push(child_pos);
        } else {
            break;
        }
    }

    children
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_parent_child_relationships() {
        assert_eq!(get_tree_parent(0, 2), None);
        assert_eq!(get_tree_parent(1, 2), Some(0));
        assert_eq!(get_tree_parent(2, 2), Some(0));
        assert_eq!(get_tree_parent(3, 2), Some(1));
        assert_eq!(get_tree_parent(4, 2), Some(1));
        assert_eq!(get_tree_parent(5, 2), Some(2));
        assert_eq!(get_tree_children(0, 2, 10), vec![1, 2]);
        assert_eq!(get_tree_children(1, 2, 10), vec![3, 4]);
        assert_eq!(get_tree_children(2, 2, 10), vec![5, 6]);

        assert_eq!(get_tree_parent(0, 3), None);
        assert_eq!(get_tree_parent(1, 3), Some(0));
        assert_eq!(get_tree_parent(2, 3), Some(0));
        assert_eq!(get_tree_parent(3, 3), Some(0));
        assert_eq!(get_tree_parent(4, 3), Some(1));
        assert_eq!(get_tree_parent(5, 3), Some(1));
        assert_eq!(get_tree_parent(6, 3), Some(1));
        assert_eq!(get_tree_parent(7, 3), Some(2));
        assert_eq!(get_tree_parent(8, 3), Some(2));
        assert_eq!(get_tree_parent(9, 3), Some(2));

        assert_eq!(get_tree_parent(0, 4), None);
        assert_eq!(get_tree_parent(1, 4), Some(0));
        assert_eq!(get_tree_parent(2, 4), Some(0));
        assert_eq!(get_tree_parent(3, 4), Some(0));
        assert_eq!(get_tree_parent(4, 4), Some(0));
        assert_eq!(get_tree_parent(5, 4), Some(1));
        assert_eq!(get_tree_parent(6, 4), Some(1));
        assert_eq!(get_tree_parent(7, 4), Some(1));
        assert_eq!(get_tree_parent(8, 4), Some(1));
        assert_eq!(get_tree_parent(9, 4), Some(2));
        assert_eq!(get_tree_parent(10, 4), Some(2));
        assert_eq!(get_tree_parent(11, 4), Some(2));
        assert_eq!(get_tree_parent(12, 4), Some(2));
        assert_eq!(get_tree_parent(13, 4), Some(3));
        assert_eq!(get_tree_parent(14, 4), Some(3));
        assert_eq!(get_tree_parent(15, 4), Some(3));
        assert_eq!(get_tree_parent(16, 4), Some(3));
        assert_eq!(get_tree_parent(17, 4), Some(4));
        assert_eq!(get_tree_parent(18, 4), Some(4));
        assert_eq!(get_tree_parent(19, 4), Some(4));
    }
}
