use TopicHash;
use libp2p_core::PeerId;
use smallvec::SmallVec;
use std::collections::hash_map::HashMap;

pub struct Peers {
    pub gossipsub: HashMap<PeerId, SmallVec<[TopicHash; 8]>>,
    pub floodsub: HashMap<PeerId, SmallVec<[TopicHash; 8]>>,
}

impl Peers {
    pub fn new() -> Self {
        Peers {
            gossipsub: HashMap::new(),
            floodsub: HashMap::new()
        }
    }
}
