//! Dynamic propeller tree computation logic.
//!
//! This module implements the core tree topology algorithms inspired by Solana's Turbine protocol.
//! The tree is computed dynamically for each shred using deterministic seeded randomization
//! based on the leader and shred ID, making the network resilient to targeted attacks.

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
    /// The shuffled peer IDs for this specific tree (leader is always at index 0).
    peers: Vec<PeerId>,
    /// Data plane fanout (number of children each node has).
    fanout: usize,
    /// This node's index in the shuffled peer IDs.
    local_index: usize,
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
    ///
    /// Note: invalidates the leader (must be set again)
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
    pub(crate) fn build_tree(
        &self,
        publisher: &PeerId,
        shred_hash: &ShredHash,
    ) -> Result<PropellerTree, TreeGenerationError> {
        let mut rng = ChaChaRng::from_seed(*shred_hash);

        let mut indices_to_choose_from: Vec<usize> = (0..self.nodes.len()).collect();
        let mut tree_indices = Vec::with_capacity(self.nodes.len());

        // remove the leader from the indices (should be at index 0 at the end)
        let leader_index = self
            .nodes
            .iter()
            .position(|node| node.peer_id == *publisher)
            .ok_or(TreeGenerationError::PublisherNotFound {
                publisher: *publisher,
            })?;
        indices_to_choose_from.remove(leader_index);
        tree_indices.push(leader_index);

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
            self.nodes.len(),
            "All indices should be present"
        );
        assert!(
            {
                let mut a = tree_indices.clone();
                a.sort();
                a.dedup();
                a.len() == self.nodes.len()
            },
            "All indices should be unique"
        );
        assert!(
            tree_indices.iter().all(|index| index < &self.nodes.len()),
            "All indices should be less than the number of nodes"
        );
        assert!(
            self.nodes[tree_indices[0]].peer_id == *publisher,
            "Leader should be at index 0"
        );

        // Convert indices to actual peer IDs
        let peers: Vec<PeerId> = tree_indices
            .into_iter()
            .map(|index| self.nodes[index].peer_id)
            .collect();

        let local_index = peers
            .iter()
            .position(|peer_id| *peer_id == self.local_peer_id)
            .ok_or(TreeGenerationError::LocalPeerNotInPeerWeights)?;
        assert_eq!(peers[local_index], self.local_peer_id);

        Ok(PropellerTree {
            peers,
            fanout: self.fanout,
            local_index,
        })
    }
}

impl PropellerTree {
    /// Get the children peer IDs for the local node in this tree.
    pub(crate) fn get_children(&self) -> Vec<PeerId> {
        get_tree_children(self.local_index, self.fanout, self.peers.len())
            .into_iter()
            .map(|index| self.peers[index])
            .collect()
    }

    /// Get the parent peer ID for the local node in this tree.
    pub(crate) fn get_parent(&self) -> Option<PeerId> {
        let parent = get_tree_parent(self.local_index, self.fanout);
        parent.map(|index| self.peers[index])
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
