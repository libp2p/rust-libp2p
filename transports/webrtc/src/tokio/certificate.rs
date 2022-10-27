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

#[derive(Clone, PartialEq)]
pub struct Certificate {
    inner: RTCCertificate,
}

impl Certificate {
    /// Generate new certificate.
    ///
    /// TODO: make use of `rng`
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
        let fingerprints = self.inner.get_fingerprints().expect("to never fail");
        let sha256_fingerprint = fingerprints
            .iter()
            .find(|f| f.algorithm == "sha-256")
            .expect("a SHA-256 fingerprint");

        Fingerprint::try_from_rtc_dtls(sha256_fingerprint).expect("we filtered by sha-256")
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
enum Kind {}
