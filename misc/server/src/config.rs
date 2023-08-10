use libp2p::Multiaddr;
use serde_derive::Deserialize;
use std::error::Error;
use std::path::Path;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Config {
    pub identity: Identity,
    pub addresses: Addresses,
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn Error>> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Identity {
    #[serde(rename = "PeerID")]
    pub peer_id: String,
    pub priv_key: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Addresses {
    pub swarm: Vec<Multiaddr>,
    pub append_announce: Vec<Multiaddr>,
}

impl zeroize::Zeroize for Config {
    fn zeroize(&mut self) {
        self.identity.peer_id.zeroize();
        self.identity.priv_key.zeroize();
    }
}
