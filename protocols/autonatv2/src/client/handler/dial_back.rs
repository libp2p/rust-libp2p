use libp2p_swarm::ConnectionHandler;

#[derive(Debug)]
pub enum ToBehaviour {
    GotNonce(u64),
}

#[derive(Debug, Default)]
pub struct Handler;

impl ConnectionHandler for Handler {
}
