// Copyright 2022 Parity Technologies (UK) Ltd.
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

use rand::{distributions::DistString, CryptoRng, Rng};
use webrtc::peer_connection::certificate::RTCCertificate;

use crate::tokio::fingerprint::Fingerprint;

#[derive(Debug, Clone, PartialEq)]
pub struct Certificate {
    inner: RTCCertificate,
}

impl Certificate {
    /// Generate new certificate.
    ///
    /// `_rng` argument is ignored for now. See <https://github.com/melekes/rust-libp2p/pull/12>.
    #[allow(clippy::unnecessary_wraps)]
    pub fn generate<R>(_rng: &mut R) -> Result<Self, Error>
    where
        R: CryptoRng + Rng,
    {
        let mut params = rcgen::CertificateParams::new(vec![
            rand::distributions::Alphanumeric.sample_string(&mut rand::thread_rng(), 16)
        ]);
        params.alg = &rcgen::PKCS_ECDSA_P256_SHA256;
        Ok(Self {
            inner: RTCCertificate::from_params(params).expect("default params to work"),
        })
    }

    /// Returns SHA-256 fingerprint of this certificate.
    ///
    /// # Panics
    ///
    /// This function will panic if there's no fingerprint with the SHA-256 algorithm (see
    /// [`RTCCertificate::get_fingerprints`]).
    pub fn fingerprint(&self) -> Fingerprint {
        let fingerprints = self.inner.get_fingerprints();
        let sha256_fingerprint = fingerprints
            .iter()
            .find(|f| f.algorithm == "sha-256")
            .expect("a SHA-256 fingerprint");

        Fingerprint::try_from_rtc_dtls(sha256_fingerprint).expect("we filtered by sha-256")
    }

    /// Parses a certificate from the ASCII PEM format.
    ///
    /// See [`RTCCertificate::from_pem`]
    #[cfg(feature = "pem")]
    pub fn from_pem(pem_str: &str) -> Result<Self, Error> {
        Ok(Self {
            inner: RTCCertificate::from_pem(pem_str).map_err(Kind::InvalidPEM)?,
        })
    }

    /// Serializes the certificate (including the private key) in PKCS#8 format in PEM.
    ///
    /// See [`RTCCertificate::serialize_pem`]
    #[cfg(feature = "pem")]
    pub fn serialize_pem(&self) -> String {
        self.inner.serialize_pem()
    }

    /// Extract the [`RTCCertificate`] from this wrapper.
    ///
    /// This function is `pub(crate)` to avoid leaking the `webrtc` dependency to our users.
    pub(crate) fn to_rtc_certificate(&self) -> RTCCertificate {
        self.inner.clone()
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Failed to generate certificate")]
pub struct Error(#[from] Kind);

#[derive(thiserror::Error, Debug)]
enum Kind {
    #[error(transparent)]
    InvalidPEM(#[from] webrtc::Error),
}

#[cfg(all(test, feature = "pem"))]
mod test {
    use rand::thread_rng;

    use super::*;

    #[test]
    fn test_certificate_serialize_pem_and_from_pem() {
        let cert = Certificate::generate(&mut thread_rng()).unwrap();

        let pem = cert.serialize_pem();
        let loaded_cert = Certificate::from_pem(&pem).unwrap();

        assert_eq!(loaded_cert, cert)
    }
}
