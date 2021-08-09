use ed25519_dalek::{Keypair, PublicKey};
use libp2p::PeerId;
use quinn_proto::crypto::Session;
use quinn_proto::TransportConfig;
use std::sync::Arc;

pub struct CryptoConfig<K> {
    pub keypair: Keypair,
    pub psk: Option<[u8; 32]>,
    pub keylogger: Option<K>,
    pub transport: Arc<TransportConfig>,
}

#[cfg(feature = "noise")]
impl<K> CryptoConfig<K> {
    fn clone_keypair(&self) -> Keypair {
        Keypair::from_bytes(&self.keypair.to_bytes()).expect("serde works")
    }
}

pub trait Crypto: std::fmt::Debug + Clone + 'static {
    type Session: Session + Unpin;
    type Keylogger: Send + Sync;

    fn new_server_config(
        config: &Arc<CryptoConfig<Self::Keylogger>>,
    ) -> <Self::Session as Session>::ServerConfig;
    fn new_client_config(
        config: &Arc<CryptoConfig<Self::Keylogger>>,
        remote_public: PublicKey,
    ) -> <Self::Session as Session>::ClientConfig;
    fn supported_quic_versions() -> Vec<u32>;
    fn default_quic_version() -> u32;
    fn peer_id(session: &Self::Session) -> Option<PeerId>;
    fn keylogger() -> Self::Keylogger;
}

#[cfg(feature = "noise")]
#[derive(Clone, Copy, Debug)]
pub struct NoiseCrypto;

#[cfg(feature = "noise")]
impl Crypto for NoiseCrypto {
    type Session = quinn_noise::NoiseSession;
    type Keylogger = Arc<dyn quinn_noise::KeyLog>;

    fn new_server_config(
        config: &Arc<CryptoConfig<Self::Keylogger>>,
    ) -> <Self::Session as Session>::ServerConfig {
        Arc::new(
            quinn_noise::NoiseServerConfig {
                keypair: config.clone_keypair(),
                psk: config.psk,
                keylogger: config.keylogger.clone(),
                supported_protocols: vec![b"libp2p".to_vec()],
            }
            .into(),
        )
    }

    fn new_client_config(
        config: &Arc<CryptoConfig<Self::Keylogger>>,
        remote_public_key: PublicKey,
    ) -> <Self::Session as Session>::ClientConfig {
        quinn_noise::NoiseClientConfig {
            keypair: config.clone_keypair(),
            psk: config.psk,
            alpn: b"libp2p".to_vec(),
            remote_public_key,
            keylogger: config.keylogger.clone(),
        }
        .into()
    }

    fn supported_quic_versions() -> Vec<u32> {
        quinn_noise::SUPPORTED_QUIC_VERSIONS.to_vec()
    }

    fn default_quic_version() -> u32 {
        quinn_noise::DEFAULT_QUIC_VERSION
    }

    fn peer_id(session: &Self::Session) -> Option<PeerId> {
        use crate::ToLibp2p;
        Some(session.peer_identity()?.to_peer_id())
    }

    fn keylogger() -> Self::Keylogger {
        Arc::new(quinn_noise::KeyLogFile::new())
    }
}

#[cfg(feature = "tls")]
#[derive(Clone, Copy, Debug)]
pub struct TlsCrypto;

#[cfg(feature = "tls")]
impl Crypto for TlsCrypto {
    type Session = quinn_proto::crypto::rustls::TlsSession;
    type Keylogger = Arc<dyn rustls::KeyLog>;

    fn new_server_config(
        config: &Arc<CryptoConfig<Self::Keylogger>>,
    ) -> <Self::Session as Session>::ServerConfig {
        assert!(config.psk.is_none(), "invalid config");
        use crate::ToLibp2p;
        let mut server =
            crate::tls::make_server_config(&config.keypair.to_keypair()).expect("invalid config");
        if let Some(key_log) = config.keylogger.clone() {
            server.key_log = key_log;
        }
        Arc::new(server)
    }

    fn new_client_config(
        config: &Arc<CryptoConfig<Self::Keylogger>>,
        remote_public: PublicKey,
    ) -> <Self::Session as Session>::ClientConfig {
        assert!(config.psk.is_none(), "invalid config");
        use crate::ToLibp2p;
        let mut client = crate::tls::make_client_config(
            &config.keypair.to_keypair(),
            remote_public.to_peer_id(),
        )
        .expect("invalid config");
        if let Some(key_log) = config.keylogger.clone() {
            client.key_log = key_log;
        }
        Arc::new(client)
    }

    fn supported_quic_versions() -> Vec<u32> {
        quinn_proto::DEFAULT_SUPPORTED_VERSIONS.to_vec()
    }

    fn default_quic_version() -> u32 {
        quinn_proto::DEFAULT_SUPPORTED_VERSIONS[0]
    }

    fn peer_id(session: &Self::Session) -> Option<PeerId> {
        let certificate = session.get_peer_certificates()?.into_iter().next()?;
        Some(crate::tls::extract_peerid_or_panic(
            quinn_proto::Certificate::from(certificate).as_der(),
        ))
    }

    fn keylogger() -> Self::Keylogger {
        Arc::new(rustls::KeyLogFile::new())
    }
}
