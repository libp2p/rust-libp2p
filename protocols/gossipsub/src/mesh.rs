use std::collections::hash_map::HashMap;
use libp2p_core::PeerId;
use TopicRep;

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
pub struct Mesh { mesh: HashMap<TopicRep, Vec<PeerId>> }

impl Mesh {
    pub fn new() -> Self {
        Mesh {
            mesh: HashMap::new(),
        }
    }
}
