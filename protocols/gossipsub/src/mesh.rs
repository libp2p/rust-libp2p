use TopicHash;
use errors::GError;

use libp2p_core::PeerId;

use std::{
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

    /// Inserts a topic via it's `TopicHash` and grafted peers to the mesh.
    pub fn insert(&mut self, k: TopicHash, v: Vec<PeerId>)
        -> Option<Vec<PeerId>> {
        self.m.insert(k, v)
    }

    /// Gets all the peers that are grafted to a topic in the mesh, or returns
    /// None if the topic is not in the mesh.
    pub fn get_peers_from_topic(&self, th: &TopicHash)
        -> Result<Vec<PeerId>, GError>
    {
        let th_str = th.into_string();
        match self.m.get(th) {
            Some(peers) => {return Ok(peers.to_vec());},
            None => {return Err(GError::TopicNotInMesh{t_hash: th_str,
                err: "Tried to get peers from the topic with topic hash /
                '{th_str}' but this topic is not found in the mesh."
                .to_string()});}
        }
    }

    /// Gets a peer that is grafted to a topic in the mesh, or returns a
    /// `GError` if the peer or topic is not in the mesh.
    pub fn get_peer_from_topic(&self, th: &TopicHash, p: &PeerId)
        -> Result<PeerId, GError> {
        let get_result = self.get_peers_from_topic(th).map(|peers| {
            for peer in peers {
                if peer == *p {
                    return Ok(peer);
                }
            }
            let th_str = th.into_string();
            Err(GError::NotGraftedToTopic{t_hash: th_str,
                peer_id: p.clone().to_base58(),
                err: "Tried to get peer '{p}' but it was not found \
                in the peers that are grafted to the topic with topic hash \
                '{th_str}'.".to_string()})
        });
        match get_result {
            Ok(result) => result,
            Err(err) => Err(err),
        }
    }

    pub fn get_mut(&mut self, ) {}

    pub fn remove(&mut self, th: &TopicHash) -> Result<Vec<PeerId>, GError>
    {
        if let Some(peers) = self.m.remove(th) {
            Ok(peers)
        } else {
            let th_str = th.into_string();
            Err(GError::TopicNotInMesh{t_hash: th_str,
            err: "Tried to remove the topic with topic hash '{th_str}' from \
            the mesh.".to_string()})
        }
    }

    pub fn remove_peer_from_topic(&mut self, th: &TopicHash,
        p: &PeerId) -> Result<(), GError>
    {
        let peer_str = &(*p.to_base58());
        let th_str = th.into_string();
        match self.remove(th) {
            Ok(peers) => {
                // TODO: use remove_item when stable:
                // https://github.com/rust-lang/rust/issues/40062
                for (pos, peer) in peers.iter().enumerate() {
                    if peer == p {
                        peers.remove(pos);
                        // The same peer ID cannot exist more than
                        // once in the vector, since we check if the peer
                        // already exists before adding it in `layer::graft_many`.
                        return Ok(());
                    }
                }
                return Err(GError::NotGraftedToTopic{
                    t_hash: th_str, peer_id: peer_str.to_string(), err:
                    "Tried to remove the peer '{peer_str}' from the topic \
                    with topic hash '{th_str}'.".to_string()});
            },
            Err(GError::TopicNotInMesh{t_hash: th_str,
                err: "Tried to remove the topic with topic hash '{th_str}' \
                from the mesh.".to_string()}) => {
                return Err(GError::TopicNotInMesh{t_hash: th_str,
                err: "Tried to remove the peer with id '{peer_str}' from the \
                topic with topic hash '{th_str}' from the mesh, but the \
                topic was not found.".to_string()})
            },
        }
    }
}
