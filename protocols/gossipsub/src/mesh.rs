use TopicHash;
use errors::GError;

use libp2p_core::PeerId;

use std::{
    borrow::Borrow,
    hash::Hash,
    collections::hash_map::HashMap
    };

/// A soft overlay network for topics of interest, which meshes as a map
/// of topics to lists of peers. It is a randomized topic mesh as a map of a
/// topic to a list of peers. The local peer maintains a topic view of its
/// direct peers only, a subset of the total peers that subscribe to a topic,
/// in order to limit bandwidth and increase decentralization, security and
/// sustainability. Extracts from the
/// [spec](https://github.com/libp2p/specs/tree/master/pubsub/gossipsub)
/// are as follows, although FMI read the full spec:
/// > The overlay is maintained by exchanging subscription control messages
/// > whenever there is a change in the topic list. The subscription
/// > messages are not propagated further, so each peer maintains a topic
/// > view of its direct peers only. Whenever a peer disconnects, it is
/// > removed from the overlay…
/// > We can form an overlay mesh where each peer forwards to a subset of
/// > its peers on a stable basis… Each peer maintains its own view of the
/// > mesh for each topic, which is a list of bidirectional links to other
/// > peers. That is, in steady state, whenever a peer A is in the mesh of
/// > peer B, then peer B is also in the mesh of peer A.
///
/// > **Note**: as discussed in the spec, ambient peer discovery is pushed
/// > outside the scope of the protocol.
#[derive(Debug)]
pub struct Mesh { m: HashMap<TopicHash, Vec<PeerId>> }

impl Mesh {
    /// Creates a new `Mesh`.
    pub fn new() -> Self {
        Mesh {
            m: HashMap::new(),
        }
    }

    pub fn insert(&mut self, k: TopicHash, v: Vec<PeerId>)
        -> Option<Vec<PeerId>> {
        self.m.insert(k, v)
    }

    pub fn get_peers_from_topic<Q: ?Sized>(&self, k: &Q) -> Option<Vec<PeerId>>
    where
        TopicHash: Borrow<Q>,
        Q: Hash + Eq,
    {
        self.m.get(k)
    }

    pub fn get_peer_from_topic(&self, k: &Q, p: &PeerId) -> Option<PeerId>
    where
        TopicHash: Borrow<Q>,
        Q: Hash + Eq,
    {
        let peers = self.get(k);
        for peer in peers {
            if peer = *p {
                return Some(peer)
            }
        }
        None
    }

    pub fn get_mut(&mut self, ) {}

    pub fn remove(&mut self, th: &TopicHash) -> Result<Vec<PeerId>, GError>
    {
        match self.m.remove(th) {
            Some(peers) => peers,
            None => return Err(GError::TopicNotInMesh(th_str, "Tried to remove the topic with topic hash '{th_str}' from the mesh."))
        }
    }

    pub fn remove_peer_from_topic(&mut self, th: &TopicHash,
        p: &PeerId) -> Result<(), GError>
    {
        match self.remove(th) {
            peers => peers,
            GError::TopicNotInMesh => return GError::TopicNotInMesh,
            _ => _,
        }

        // TODO: use remove_item when stable:
        // https://github.com/rust-lang/rust/issues/40062
        for (pos, peer) in peers.iter().enumerate() {
            if peer == *p {
                peers.remove(pos);
                // Assume that the same peer ID cannot exist more than once in
                // the vector.
                return Ok(())
            } else {
                let peer_str = peer.to_base58()
                return Err(GError::NotGraftedToTopic(th_str, peer,
                    "Tried to remove the peer '{peer_str}' from the topic \
                    with topic hash '{th_str}'."))
            }
        }
        Ok(())
    }
}
