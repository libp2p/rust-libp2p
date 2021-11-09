// Copyright 2021 David Craven.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use libp2p_core::PeerId;
use quinn_proto::crypto::Session;
use quinn_proto::TransportConfig;
use std::sync::Arc;

pub struct CryptoConfig<C: Crypto> {
    pub keypair: C::Keypair,
    pub keylogger: Option<C::Keylogger>,
    pub transport: Arc<TransportConfig>,
}

pub trait ToLibp2p {
    fn to_public(&self) -> libp2p_core::identity::PublicKey;
    fn to_peer_id(&self) -> PeerId {
        self.to_public().to_peer_id()
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
        config: &CryptoConfig<Self>,
    ) -> <Self::Session as Session>::ServerConfig;
    fn new_client_config(
        config: &CryptoConfig<Self>,
        remote_public: Self::PublicKey,
    ) -> <Self::Session as Session>::ClientConfig;
    fn supported_quic_versions() -> Vec<u32>;
    fn default_quic_version() -> u32;
    fn peer_id(session: &Self::Session) -> Option<PeerId>;
    fn extract_public_key(generic_key: libp2p_core::PublicKey) -> Option<Self::PublicKey>;
    fn keylogger() -> Self::Keylogger;
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
        config: &CryptoConfig<Self>,
    ) -> <Self::Session as Session>::ServerConfig {
        let mut server = crate::tls::make_server_config(&config.keypair).expect("invalid config");
        if let Some(key_log) = config.keylogger.clone() {
            server.key_log = key_log;
        }
        Arc::new(server)
    }

    fn new_client_config(
        config: &CryptoConfig<Self>,
        remote_public: Self::PublicKey,
    ) -> <Self::Session as Session>::ClientConfig {
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
