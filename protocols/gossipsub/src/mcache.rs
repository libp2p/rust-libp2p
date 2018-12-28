use message::{MsgRepEnum, GMessage};

use std::collections::hash_map::HashMap;

/// The message cache used to track recently seen messages. FMI see
/// https://github.com/libp2p/specs/tree/master/pubsub/gossipsub#router-state.
pub struct MCache {
    msgs: Vec<HashMap<MsgRepEnum, GMessage>>,
}

impl MCache {
    pub fn new() -> Self {
        MCache {
            msgs: Vec::new(),
        }
    }

    pub fn put(&mut self, m: GMessage) {
        m.msgs.p
    }
}
