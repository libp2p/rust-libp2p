use message::MsgRepEnum;

/// The message cache used to track recently seen messages. FMI see
/// https://github.com/libp2p/specs/tree/master/pubsub/gossipsub#router-state.
pub struct MCache {
    msgs: Vec<MsgRepEnum>,
}

impl MCache {
    pub fn new() -> Self {
        MCache {
            msgs: Vec::new(),
        }
    }
}
