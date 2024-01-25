// Copyright 2020 Parity Technologies (UK) Ltd.
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

use std::sync::Arc;

use rustls;
use sha2::Digest;
use time::{Duration, OffsetDateTime};

use libp2p_core::multihash::Multihash;
use libp2p_tls::{
    certificate, P2P_ALPN, verifier,
};

const MULTIHASH_SHA256_CODE: u64 = 0x12;
const CERT_VALID_PERIOD: Duration = Duration::days(14);

#[derive(Clone, Debug)]
pub(crate) struct ServerCertManager {
    keypair: libp2p_identity::Keypair,

    items: Vec<CertItem>,
}

#[derive(Clone, Debug)]
struct CertItem {
    server_tls_config: Arc<rustls::ServerConfig>,
    start: OffsetDateTime,
    end: OffsetDateTime,
    cert_hash: Multihash<64>,
}

impl ServerCertManager {
    pub(crate) fn new(keypair: libp2p_identity::Keypair) -> Self {
        Self {
            keypair,
            items: Vec::with_capacity(3),
        }
    }

    /// Gets TLS server config and certificate hashes.
    pub(crate) fn get_config(&mut self) -> Result<(Arc<rustls::ServerConfig>, Vec<Multihash<64>>), certificate::GenError> {
        self.check_and_roll_items()?;

        let cur_item = self.items.first()
            .expect("The first element exists");
        let cert_hashes = self.items.iter()
            .map(|item| item.cert_hash.clone())
            .collect();

        Ok((cur_item.server_tls_config.clone(), cert_hashes))
    }

    fn create_cert_item(&self, start: OffsetDateTime) -> Result<CertItem, certificate::GenError> {
        let not_after = start.clone()
            .checked_add(CERT_VALID_PERIOD)
            .expect("Addition does not overflow");

        let (cert, private_key) =
            certificate::generate_webtransport_certificate(&self.keypair, start, not_after)?;

        let cert_hash = Multihash::wrap(
            MULTIHASH_SHA256_CODE, sha2::Sha256::digest(cert.as_ref().as_ref()).as_ref(),
        ).expect("fingerprint's len to be 32 bytes");

        let mut tls_config = rustls::ServerConfig::builder()
            .with_cipher_suites(verifier::CIPHERSUITES)
            .with_safe_default_kx_groups()
            .with_protocol_versions(verifier::PROTOCOL_VERSIONS)
            .expect("Cipher suites and kx groups are configured; qed")
            .with_client_cert_verifier(Arc::new(verifier::Libp2pCertificateVerifier::new()))
            .with_single_cert(vec![cert], private_key)
            .expect("Server cert key DER is valid; qed");

        tls_config.alpn_protocols = alpn_protocols();

        Ok(CertItem { server_tls_config: Arc::new(tls_config), start, end: not_after, cert_hash })
    }

    /// https://github.com/libp2p/specs/tree/master/webtransport#certificates
    /// Servers need to take care of regularly renewing their certificates.At first boot of the node,
    /// it creates one self-signed certificate with a validity of 14 days, starting immediately,
    /// and another certificate with the 14 day validity period starting on the expiry date of the first certificate.
    /// The node advertises a multiaddress containing the certificate hashes of these two certificates.
    /// Once the first certificate has expired, the node starts using the already generated next certificate.
    /// At the same time, it again generates a new certificate for the following period and updates the multiaddress it advertises.
    fn check_and_roll_items(&mut self) -> Result<(), certificate::GenError> {
        if self.items.len() == 0 {
            let current = self.create_cert_item(OffsetDateTime::now_utc())?;
            let next_start = current.end.clone();
            self.items.push(current);
            self.items.push(self.create_cert_item(next_start)?);
        } else {
            let next = self.items.get(1)
                .expect("Element with index 1 exists");

            if OffsetDateTime::now_utc() >= next.start {
                let next_start = next.end.clone();
                self.items.push(self.create_cert_item(next_start)?);
                if self.items.len() == 3 {
                    self.items.remove(0);
                }
            }
        };

        Ok(())
    }
}

fn alpn_protocols() -> Vec<Vec<u8>> {
    vec![P2P_ALPN.to_vec(),
         b"h3".to_vec(),
         b"h3-32".to_vec(),
         b"h3-31".to_vec(),
         b"h3-30".to_vec(),
         b"h3-29".to_vec(), ]
}

/*
#[cfg(test)]
mod tests {
    use std::fmt::{Debug, Formatter};
    use rcgen::SerialNumber;
    use ring::rand::{SecureRandom, SystemRandom};
    use ring::{hkdf, signature};
    use ring::error::Unspecified;
    use ring::signature::EcdsaKeyPair;
    use time::macros::datetime;

    #[test]
    fn key_pair_generate() {
        let alg = &signature::ECDSA_P256_SHA256_ASN1_SIGNING;
        let rnd = SystemRandom::new();

        let doc = EcdsaKeyPair::generate_pkcs8(alg, &rnd);

        assert!(doc.is_ok())
    }
}*/