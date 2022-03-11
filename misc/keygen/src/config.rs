use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::Path;

use libp2p_core::identity::Keypair;
use libp2p_core::PeerId;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Config {
    pub identity: Identity,
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn Error>> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn from_key_material(peer_id: PeerId, keypair: &Keypair) -> Result<Self, Box<dyn Error>> {
        let priv_key = base64::encode(keypair.to_protobuf_encoding()?);
        let peer_id = peer_id.to_base58();
        Ok(Self {
            identity: Identity { peer_id, priv_key },
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Identity {
    #[serde(rename = "PeerID")]
    pub peer_id: String,
    pub priv_key: String,
}

impl zeroize::Zeroize for Config {
    fn zeroize(&mut self) {
        self.identity.peer_id.zeroize();
        self.identity.priv_key.zeroize();
    }
}
