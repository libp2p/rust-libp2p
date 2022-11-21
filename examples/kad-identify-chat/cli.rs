use std::net::Ipv4Addr;

use clap::Parser;
use libp2p::identity::{ed25519, Keypair};
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use libp2p_core::PeerId;

// Peer tcp port will be determined by adding their peer_no to this value
const LISTEN_PORT_BASE: u16 = 63000;

#[derive(Parser, Debug)]
#[clap(name = "MyChat networking example")]
pub struct MyChatCliArgs {
    /// Fixed value to generate deterministic peer ID.
    #[clap(long, short)]
    pub peer_no: u8,

    /// Fixed value to generate deterministic peer ID and port
    /// e.g. peer_no 1 will always result in
    #[clap(long, short)]
    pub bootstrap_peer_no: Option<u8>,
}

pub fn listen_address_with_peer_id(bootstrap_peer_no: u8) -> Multiaddr {
    let mut addr = local_listen_address_from_peer_no(bootstrap_peer_no);
    addr.push(Protocol::P2p(peer_no_to_peer_id(bootstrap_peer_no).into()));
    addr
}

pub fn peer_no_to_peer_id(peer_no: u8) -> PeerId {
    keypair_from_peer_no(peer_no)
        .public()
        .to_peer_id()
}

// Deterministically create a local listen address from the peer number
pub fn local_listen_address_from_peer_no(peer_no: u8) -> Multiaddr {
    let mut list_address = Multiaddr::from(Protocol::Ip4(Ipv4Addr::LOCALHOST));
    list_address.push(Protocol::Tcp(LISTEN_PORT_BASE + peer_no as u16));
    list_address
}

// Deterministically create a keypair from the peer number
pub fn keypair_from_peer_no(peer_no: u8) -> Keypair {
    // We can unwrap here, because `SecretKey::from_bytes()` can only fail on bad length of
    // bytes slice. The  length is hardcoded to correct length of 32 here.
    Keypair::Ed25519(ed25519::Keypair::from(
        ed25519::SecretKey::from_bytes([peer_no; 32]).unwrap(),
    ))
}