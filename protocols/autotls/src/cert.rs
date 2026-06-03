use rcgen::{CertificateParams, DistinguishedName, KeyPair, PKCS_ECDSA_P256_SHA256};
use x509_parser::prelude::*;

/// Errors produced while handling certificate material.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Generating or serializing the certificate key or CSR failed.
    #[error("certificate generation failed")]
    Rcgen(#[from] rcgen::Error),
    /// The PEM certificate chain could not be parsed.
    #[error("failed to parse PEM certificate chain")]
    Pem(#[source] std::io::Error),
    /// The certificate chain contained no certificates.
    #[error("the certificate chain was empty")]
    EmptyChain,
    /// A certificate in the chain could not be parsed as X.509.
    #[error("failed to parse X.509 certificate")]
    X509,
}

/// The private key used for the AutoTLS certificate, distinct from the node's identity key.
pub struct CertKey {
    keypair: KeyPair,
}

impl CertKey {
    /// Generate a fresh P-256 ECDSA certificate key.
    pub fn generate() -> Result<Self, Error> {
        Ok(Self {
            keypair: KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?,
        })
    }

    /// Load a certificate key from its PKCS#8 PEM encoding.
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self, Error> {
        Ok(Self {
            keypair: KeyPair::from_pem(pem)?,
        })
    }

    /// Serialize the certificate key as PKCS#8 PEM, for persistence.
    pub fn to_pkcs8_pem(&self) -> String {
        self.keypair.serialize_pem()
    }

    /// Build a DER-encoded PKCS#10 certificate signing request for the given (wildcard) domain.
    pub fn certificate_signing_request(&self, domain: &str) -> Result<Vec<u8>, Error> {
        let mut params = CertificateParams::new(vec![domain.to_owned()])?;
        params.distinguished_name = DistinguishedName::new();
        Ok(params.serialize_request(&self.keypair)?.der().to_vec())
    }
}

/// The Unix timestamp at which the leaf certificate of the chain expires, used to
/// schedule renewal.
pub fn not_after_unix(chain_pem: &str) -> Result<i64, Error> {
    let mut reader = chain_pem.as_bytes();
    let chain = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::Pem)?;
    let leaf = chain.first().ok_or(Error::EmptyChain)?;
    let (_, certificate) = X509Certificate::from_der(leaf).map_err(|_| Error::X509)?;
    Ok(certificate.validity().not_after.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOMAIN: &str =
        "*.k51qzi5uqu5diuci8bva7narzo109juvlfbckhzf3j2ljua2979b21rs6uyquk.libp2p.direct";

    fn self_signed_chain(cert_key: &CertKey) -> String {
        let params = CertificateParams::new(vec![DOMAIN.to_owned()]).unwrap();
        params.self_signed(&cert_key.keypair).unwrap().pem()
    }

    #[test]
    fn key_pem_round_trips() {
        let key = CertKey::generate().unwrap();
        let reloaded = CertKey::from_pkcs8_pem(&key.to_pkcs8_pem()).unwrap();
        assert!(reloaded.certificate_signing_request(DOMAIN).is_ok());
    }

    #[test]
    fn csr_is_valid_der() {
        let key = CertKey::generate().unwrap();
        let csr = key.certificate_signing_request(DOMAIN).unwrap();
        assert!(!csr.is_empty());
        assert_eq!(csr[0], 0x30, "DER PKCS#10 must start with a SEQUENCE tag");
    }

    #[test]
    fn reads_expiry() {
        let key = CertKey::generate().unwrap();
        let chain = self_signed_chain(&key);
        assert!(not_after_unix(&chain).unwrap() > 0);
    }

    #[test]
    fn empty_chain_is_rejected() {
        assert!(matches!(not_after_unix(""), Err(Error::EmptyChain)));
    }
}
