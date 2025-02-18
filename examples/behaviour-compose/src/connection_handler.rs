use derive_more::From;
use libp2p::{
    identify, mdns, ping,
    swarm::{ConnectionHandler, NetworkBehaviour},
};

pub struct Handler {
    pub ping: <ping::Behaviour as NetworkBehaviour>::ConnectionHandler,
    pub identify: <identify::Behaviour as NetworkBehaviour>::ConnectionHandler,
    pub mdns: <mdns::tokio::Behaviour as NetworkBehaviour>::ConnectionHandler,
}

#[derive(From)]
pub enum ToBehaviour {
    #[from]
    Ping(<<ping::Behaviour as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::ToBehaviour),
    #[from]
    Identify(<<ping::Behaviour as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::ToBehaviour),
    #[from]
    Mdns(<<ping::Behaviour as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::ToBehaviour),
}
