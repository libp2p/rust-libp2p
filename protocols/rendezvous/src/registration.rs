use libp2p_core::PeerId;
use std::collections::{HashMap, HashSet};

pub struct RegistrationStore {
    registrations: HashMap<String, HashSet<PeerId>>,
}

impl RegistrationStore {
    pub fn register(&self, registration: String, peer: PeerId) {
        if let Some(peers) = self.registrations.get(&registration) {}
    }
}
