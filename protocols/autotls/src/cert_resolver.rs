use std::{fmt, sync::Arc};

use arc_swap::ArcSwapOption;
use rustls::{
    crypto::ring::sign::any_supported_type,
    pki_types::{
        CertificateDer, PrivateKeyDer,
        pem::{self, PemObject},
    },
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};

/// Errors installing a certificate into the resolver.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The PEM could not be parsed.
    #[error("failed to parse PEM")]
    Pem(#[source] pem::Error),
    /// The key PEM contained no private key.
    #[error("the PEM contained no private key")]
    MissingKey,
    /// The private key could not be used as a `rustls` signing key.
    #[error("invalid certificate signing key")]
    SigningKey(#[source] rustls::Error),
}

/// A [`ResolvesServerCert`] whose certificate can be replaced at runtime.
#[derive(Clone, Default)]
pub struct AutoTlsCertResolver(Arc<ArcSwapOption<CertifiedKey>>);

impl AutoTlsCertResolver {
    /// Create a resolver that serves no certificate until one is installed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the PEM certificate chain and PKCS#8 PEM key, replacing any previous certificate.
    pub fn set_pem(&self, chain_pem: &str, key_pem: &str) -> Result<(), Error> {
        let chain = CertificateDer::pem_slice_iter(chain_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::Pem)?;
        let key =
            PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).map_err(|error| match error {
                pem::Error::NoItemsFound => Error::MissingKey,
                error => Error::Pem(error),
            })?;
        let signing_key = any_supported_type(&key).map_err(Error::SigningKey)?;
        self.0
            .store(Some(Arc::new(CertifiedKey::new(chain, signing_key))));
        Ok(())
    }

    /// Whether a certificate is currently installed.
    pub fn is_set(&self) -> bool {
        self.0.load().is_some()
    }
}

impl fmt::Debug for AutoTlsCertResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AutoTlsCertResolver")
            .field("is_set", &self.is_set())
            .finish()
    }
}

impl ResolvesServerCert for AutoTlsCertResolver {
    fn resolve(&self, _client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.0.load_full()
    }
}
