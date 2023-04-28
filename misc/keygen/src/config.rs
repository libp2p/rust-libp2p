use base64::prelude::*;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::Path;

use libp2p_identity::Keypair;
use libp2p_identity::PeerId;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Config {
    pub(crate) identity: Identity,
}

impl Config {
    pub(crate) fn from_file(path: &Path) -> Result<Self, Box<dyn Error>> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub(crate) fn from_key_material(
        peer_id: PeerId,
        keypair: &Keypair,
    ) -> Result<Self, Box<dyn Error>> {
        let priv_key = BASE64_STANDARD.encode(keypair.to_protobuf_encoding()?);
        let peer_id = peer_id.to_base58();
        Ok(Self {
            identity: Identity { peer_id, priv_key },
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct Identity {
    #[serde(rename = "PeerID")]
    pub(crate) peer_id: String,
    pub(crate) priv_key: String,
}

impl zeroize::Zeroize for Config {
    fn zeroize(&mut self) {
        self.identity.peer_id.zeroize();
        self.identity.priv_key.zeroize();
    }
}
