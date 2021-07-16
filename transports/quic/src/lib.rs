mod endpoint;
mod muxer;
mod transport;

pub use crate::muxer::{QuicMuxer, QuicMuxerError};
pub use crate::transport::{QuicDial, QuicTransport};
pub use quinn_noise::{KeyLog, KeyLogFile, Keypair, PublicKey, SecretKey};
pub use quinn_proto::{ConfigError, ConnectError, ConnectionError, TransportConfig};

use libp2p_core::identity;
use libp2p_core::transport::TransportError;
use libp2p_core::{Multiaddr, PeerId};
use std::sync::Arc;
use thiserror::Error;

pub fn generate_keypair() -> Keypair {
    Keypair::generate(&mut rand_core::OsRng {})
}

/// Quic configuration.
pub struct QuicConfig {
    pub keypair: Keypair,
    pub psk: Option<[u8; 32]>,
    pub transport: TransportConfig,
    pub keylogger: Option<Arc<dyn KeyLog>>,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            keypair: Keypair::generate(&mut rand_core::OsRng {}),
            psk: None,
            transport: TransportConfig::default(),
            keylogger: None,
        }
    }
}

impl std::fmt::Debug for QuicConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("QuicConfig")
            .field("keypair", &self.keypair.public)
            .field("psk", &self.psk)
            .field("transport", &self.transport)
            .finish()
    }
}

impl QuicConfig {
    /// Creates a new config from a keypair.
    pub fn new(keypair: Keypair) -> Self {
        Self {
            keypair,
            ..Default::default()
        }
    }

    /// Enable keylogging.
    pub fn enable_keylogger(&mut self) -> &mut Self {
        self.keylogger = Some(Arc::new(KeyLogFile::new()));
        self
    }

    /// Spawns a new endpoint.
    pub async fn listen_on(
        self,
        addr: Multiaddr,
    ) -> Result<QuicTransport, TransportError<QuicError>> {
        QuicTransport::new(self, addr).await
    }
}

#[derive(Debug, Error)]
pub enum QuicError {
    #[error("{0}")]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Connect(#[from] ConnectError),
    #[error("{0}")]
    Muxer(#[from] QuicMuxerError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("a `StreamMuxerEvent` was generated before the handshake was complete.")]
    UpgradeError,
}

pub trait ToLibp2p {
    fn to_keypair(&self) -> identity::Keypair;
    fn to_public(&self) -> identity::PublicKey;
    fn to_peer_id(&self) -> PeerId {
        self.to_public().into_peer_id()
    }
}

impl ToLibp2p for Keypair {
    fn to_keypair(&self) -> identity::Keypair {
        let mut secret_key = self.secret.to_bytes();
        let secret_key = identity::ed25519::SecretKey::from_bytes(&mut secret_key).unwrap();
        identity::Keypair::Ed25519(secret_key.into())
    }

    fn to_public(&self) -> identity::PublicKey {
        self.public.to_public()
    }
}

impl ToLibp2p for PublicKey {
    fn to_keypair(&self) -> identity::Keypair {
        panic!("wtf?");
    }

    fn to_public(&self) -> identity::PublicKey {
        let public_key = self.to_bytes();
        let public_key = identity::ed25519::PublicKey::decode(&public_key[..]).unwrap();
        identity::PublicKey::Ed25519(public_key.into())
    }
}
