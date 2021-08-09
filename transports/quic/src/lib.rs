mod crypto;
mod endpoint;
mod muxer;
#[cfg(feature = "tls")]
mod tls;
mod transport;

pub use crate::crypto::Crypto;
#[cfg(feature = "noise")]
pub use crate::crypto::NoiseCrypto;
#[cfg(feature = "tls")]
pub use crate::crypto::TlsCrypto;
pub use crate::muxer::{QuicMuxer, QuicMuxerError};
pub use crate::transport::{QuicDial, QuicTransport};
pub use ed25519_dalek::{Keypair, PublicKey, SecretKey};
#[cfg(feature = "noise")]
pub use quinn_noise::{KeyLog, KeyLogFile};
pub use quinn_proto::{ConfigError, ConnectError, ConnectionError, TransportConfig};

use libp2p::core::transport::TransportError;
use libp2p::{Multiaddr, PeerId};
use quinn_proto::crypto::Session;
use thiserror::Error;

pub fn generate_keypair() -> Keypair {
    Keypair::generate(&mut rand_core::OsRng {})
}

/// Quic configuration.
pub struct QuicConfig<C: Crypto> {
    pub keypair: Keypair,
    pub psk: Option<[u8; 32]>,
    pub transport: TransportConfig,
    pub keylogger: Option<C::Keylogger>,
}

impl<C: Crypto> Default for QuicConfig<C> {
    fn default() -> Self {
        Self {
            keypair: Keypair::generate(&mut rand_core::OsRng {}),
            psk: None,
            transport: TransportConfig::default(),
            keylogger: None,
        }
    }
}

impl<C: Crypto> std::fmt::Debug for QuicConfig<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("QuicConfig")
            .field("keypair", &self.keypair.public)
            .field("psk", &self.psk)
            .field("transport", &self.transport)
            .finish()
    }
}

impl<C: Crypto> QuicConfig<C>
where
    <C::Session as Session>::ClientConfig: Send + Unpin,
    <C::Session as Session>::HeaderKey: Unpin,
    <C::Session as Session>::PacketKey: Unpin,
{
    /// Creates a new config from a keypair.
    pub fn new(keypair: Keypair) -> Self {
        Self {
            keypair,
            ..Default::default()
        }
    }

    /// Enable keylogging.
    pub fn enable_keylogger(&mut self) -> &mut Self {
        self.keylogger = Some(C::keylogger());
        self
    }

    /// Spawns a new endpoint.
    pub async fn listen_on(
        self,
        addr: Multiaddr,
    ) -> Result<QuicTransport<C>, TransportError<QuicError>> {
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
    fn to_keypair(&self) -> libp2p::identity::Keypair;
    fn to_public(&self) -> libp2p::identity::PublicKey;
    fn to_peer_id(&self) -> PeerId {
        self.to_public().into_peer_id()
    }
}

impl ToLibp2p for Keypair {
    fn to_keypair(&self) -> libp2p::identity::Keypair {
        let mut secret_key = self.secret.to_bytes();
        let secret_key = libp2p::identity::ed25519::SecretKey::from_bytes(&mut secret_key).unwrap();
        libp2p::identity::Keypair::Ed25519(secret_key.into())
    }

    fn to_public(&self) -> libp2p::identity::PublicKey {
        self.public.to_public()
    }
}

impl ToLibp2p for PublicKey {
    fn to_keypair(&self) -> libp2p::identity::Keypair {
        panic!("wtf?");
    }

    fn to_public(&self) -> libp2p::identity::PublicKey {
        let public_key = self.to_bytes();
        let public_key = libp2p::identity::ed25519::PublicKey::decode(&public_key[..]).unwrap();
        libp2p::identity::PublicKey::Ed25519(public_key)
    }
}
