use std::{error::Error, path::Path};

use libp2p::Multiaddr;
use serde_derive::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Config {
    pub(crate) identity: Identity,
    pub(crate) addresses: Addresses,
}

impl Config {
    pub(crate) fn from_file(path: &Path) -> Result<Self, Box<dyn Error>> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Identity {
    #[serde(rename = "PeerID")]
    pub(crate) peer_id: String,
    pub(crate) priv_key: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Addresses {
    pub(crate) swarm: Vec<Multiaddr>,
    pub(crate) append_announce: Vec<Multiaddr>,
}

impl zeroize::Zeroize for Config {
    fn zeroize(&mut self) {
        self.identity.peer_id.zeroize();
        self.identity.priv_key.zeroize();
    }
}
