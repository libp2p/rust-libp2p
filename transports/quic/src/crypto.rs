use libp2p_core::PeerId;
use quinn_proto::crypto::Session;
use quinn_proto::TransportConfig;
use std::sync::Arc;

pub struct CryptoConfig<C: Crypto> {
    pub keypair: C::Keypair,
    pub psk: Option<[u8; 32]>,
    pub keylogger: Option<C::Keylogger>,
    pub transport: Arc<TransportConfig>,
}

#[cfg(feature = "noise")]
trait CloneKeypair {
    fn clone_keypair(&self) -> Self;
}

#[cfg(feature = "noise")]
impl CloneKeypair for ed25519_dalek::Keypair {
    fn clone_keypair(&self) -> Self {
        ed25519_dalek::Keypair::from_bytes(&self.to_bytes()).expect("serde works")
    }
}

pub trait ToLibp2p {
    fn to_public(&self) -> libp2p_core::identity::PublicKey;
    fn to_peer_id(&self) -> PeerId {
        self.to_public().to_peer_id()
    }
}

#[cfg(feature = "noise")]
impl ToLibp2p for ed25519_dalek::Keypair {
    fn to_public(&self) -> libp2p_core::identity::PublicKey {
        self.public.to_public()
    }
}

#[cfg(feature = "noise")]
impl ToLibp2p for ed25519_dalek::PublicKey {
    fn to_public(&self) -> libp2p_core::identity::PublicKey {
        let public_key = self.to_bytes();
        let public_key =
            libp2p_core::identity::ed25519::PublicKey::decode(&public_key[..]).unwrap();
        libp2p_core::identity::PublicKey::Ed25519(public_key)
    }
}

#[cfg(feature = "tls")]
impl ToLibp2p for libp2p_core::identity::Keypair {
    fn to_public(&self) -> libp2p_core::identity::PublicKey {
        self.public()
    }
}

pub trait Crypto: std::fmt::Debug + Clone + 'static {
    type Session: Session + Unpin;
    type Keylogger: Send + Sync;
    type Keypair: Send + Sync + ToLibp2p;
    type PublicKey: Send + std::fmt::Debug + PartialEq<Self::PublicKey>;

    fn new_server_config(
        config: &Arc<CryptoConfig<Self>>,
    ) -> <Self::Session as Session>::ServerConfig;
    fn new_client_config(
        config: &Arc<CryptoConfig<Self>>,
        remote_public: Self::PublicKey,
    ) -> <Self::Session as Session>::ClientConfig;
    fn supported_quic_versions() -> Vec<u32>;
    fn default_quic_version() -> u32;
    fn peer_id(session: &Self::Session) -> Option<PeerId>;
    fn extract_public_key(generic_key: libp2p_core::PublicKey) -> Option<Self::PublicKey>;
    fn keylogger() -> Self::Keylogger;
}

#[cfg(feature = "noise")]
#[derive(Clone, Copy, Debug)]
pub struct NoiseCrypto;

#[cfg(feature = "noise")]
impl Crypto for NoiseCrypto {
    type Session = quinn_noise::NoiseSession;
    type Keylogger = Arc<dyn quinn_noise::KeyLog>;
    type Keypair = ed25519_dalek::Keypair;
    type PublicKey = ed25519_dalek::PublicKey;

    fn new_server_config(
        config: &Arc<CryptoConfig<Self>>,
    ) -> <Self::Session as Session>::ServerConfig {
        Arc::new(
            quinn_noise::NoiseServerConfig {
                keypair: config.keypair.clone_keypair(),
                psk: config.psk,
                keylogger: config.keylogger.clone(),
                supported_protocols: vec![b"libp2p".to_vec()],
            }
            .into(),
        )
    }

    fn new_client_config(
        config: &Arc<CryptoConfig<Self>>,
        remote_public_key: Self::PublicKey,
    ) -> <Self::Session as Session>::ClientConfig {
        quinn_noise::NoiseClientConfig {
            keypair: config.keypair.clone_keypair(),
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
        Some(session.peer_identity()?.to_peer_id())
    }

    fn extract_public_key(generic_key: libp2p_core::PublicKey) -> Option<Self::PublicKey> {
        let public_key = if let libp2p_core::PublicKey::Ed25519(public_key) = generic_key {
            public_key.encode()
        } else {
            return None;
        };
        Self::PublicKey::from_bytes(&public_key).ok()
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
    type Keypair = libp2p_core::identity::Keypair;
    type PublicKey = libp2p_core::identity::PublicKey;

    fn new_server_config(
        config: &Arc<CryptoConfig<Self>>,
    ) -> <Self::Session as Session>::ServerConfig {
        assert!(config.psk.is_none(), "invalid config");
        let mut server = crate::tls::make_server_config(&config.keypair).expect("invalid config");
        if let Some(key_log) = config.keylogger.clone() {
            server.key_log = key_log;
        }
        Arc::new(server)
    }

    fn new_client_config(
        config: &Arc<CryptoConfig<Self>>,
        remote_public: Self::PublicKey,
    ) -> <Self::Session as Session>::ClientConfig {
        assert!(config.psk.is_none(), "invalid config");
        let mut client =
            crate::tls::make_client_config(&config.keypair, remote_public.to_peer_id())
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

    fn extract_public_key(generic_key: libp2p_core::PublicKey) -> Option<Self::PublicKey> {
        Some(generic_key)
    }

    fn keylogger() -> Self::Keylogger {
        Arc::new(rustls::KeyLogFile::new())
    }
}
