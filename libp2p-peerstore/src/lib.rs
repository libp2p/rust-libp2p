#[macro_use] extern crate error_chain;
extern crate multiaddr;
extern crate libp2p_peer as peer;

mod memory_peerstore;
mod peerstore;
mod peer_info;

pub type TTL = std::time::Duration;
