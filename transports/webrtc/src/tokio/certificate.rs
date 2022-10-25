use crate::tokio::fingerprint::Fingerprint;
use rand::distributions::DistString;
use rand::{CryptoRng, Rng};
use rcgen::{CertificateParams, RcgenError, PKCS_ED25519};
use ring::signature::Ed25519KeyPair;
use ring::test::rand::FixedSliceSequenceRandom;
use std::cell::UnsafeCell;
use webrtc::peer_connection::certificate::RTCCertificate;

#[derive(Clone, PartialEq)]
pub struct Certificate {
    inner: RTCCertificate,
}

impl Certificate {
    /// Generate new certificate.
    ///
    /// This function is pure and will generate the same certificate provided the exact same randomness source.
    pub fn generate<R>(rng: &mut R) -> Result<Self, Error>
    where
        R: CryptoRng + Rng,
    {
        let keypair = new_keypair(rng)?;
        let alt_name = rand::distributions::Alphanumeric.sample_string(rng, 16);

        let mut params = CertificateParams::new(vec![alt_name.clone()]);
        params.alg = &PKCS_ED25519;
        params.key_pair = Some(keypair);

        let certificate = RTCCertificate::from_params(params).map_err(Kind::WebRTC)?;

        Ok(Self { inner: certificate })
    }

    pub fn fingerprint(&self) -> Fingerprint {
        let fingerprints = self.inner.get_fingerprints().expect("to never fail");
        let sha256_fingerprint = fingerprints
            .iter()
            .find(|f| f.algorithm == "sha-256")
            .expect("a SHA-256 fingerprint");

        Fingerprint::try_from_rtc_dtls(sha256_fingerprint).expect("we filtered by sha-256")
    }

    pub fn to_pem(&self) -> &str {
        self.inner.pem()
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
    Rcgen(RcgenError),
    #[error(transparent)]
    Ring(ring::error::Unspecified),
    #[error(transparent)]
    WebRTC(webrtc::Error),
}

/// Generates a new [`rcgen::KeyPair`] from the given randomness source.
///
/// This implementation uses `ring`'s [`FixedSliceSequenceRandom`] to create a deterministic randomness
/// source which allows us to fully control the resulting keypair.
///
/// [`FixedSliceSequenceRandom`] only hands out the provided byte-slices for each call to
/// [`SecureRandom::fill`] and therefore does not offer ANY guarantees about the randomess, it is
/// all under our control.
///
/// Using [`FixedSliceSequenceRandom`] over [`FixedSliceRandom`] ensures that we only ever use the
/// provided byte-slice once and don't reuse our "randomness" for different variables which could be
/// a security problem.
fn new_keypair<R>(rng: &mut R) -> Result<rcgen::KeyPair, Error>
where
    R: CryptoRng + Rng,
{
    let ring_rng = FixedSliceSequenceRandom {
        bytes: &[&rng.gen::<[u8; 32]>()],
        current: UnsafeCell::new(0),
    };

    let document = Ed25519KeyPair::generate_pkcs8(&ring_rng).map_err(Kind::Ring)?;
    let der = document.as_ref();
    let keypair = rcgen::KeyPair::from_der(der).map_err(Kind::Rcgen)?;

    Ok(keypair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::thread_rng;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn certificate_generation_is_deterministic() {
        let certificate = Certificate::generate(&mut ChaCha20Rng::from_seed([0u8; 32])).unwrap();

        let pem = certificate.to_pem();

        assert_eq!(
            pem,
            "-----BEGIN CERTIFICATE-----\r\nMIIBGTCBzKADAgECAgkA349L+6JmEFgwBQYDK2VwMCExHzAdBgNVBAMMFnJjZ2Vu\r\nIHNlbGYgc2lnbmVkIGNlcnQwIBcNNzUwMTAxMDAwMDAwWhgPNDA5NjAxMDEwMDAw\r\nMDBaMCExHzAdBgNVBAMMFnJjZ2VuIHNlbGYgc2lnbmVkIGNlcnQwKjAFBgMrZXAD\r\nIQDqCj9zLT7ijKUXLEZw7/JZeuDEvleLkSgyFN3Q8lhWXqMfMB0wGwYDVR0RBBQw\r\nEoIQNTRDZG14TlhhREhFd1k4VzAFBgMrZXADQQBQdGOd+rpYKM63TTDT7V4TysD3\r\nhSD7qs2fNQ+tM7mGe9r2mScaOXvnoCnlLt/wDsEB3hFwpkmRbgZMLjCooZ8E\r\n-----END CERTIFICATE-----\r\n"
        )
    }

    #[test]
    fn cloned_certificate_is_equivalent() {
        let certificate = Certificate::generate(&mut thread_rng()).unwrap();

        let cloned_certificate = certificate.clone();

        assert!(certificate == cloned_certificate)
    }
}
