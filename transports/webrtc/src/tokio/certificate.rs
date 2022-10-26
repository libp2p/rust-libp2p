use crate::tokio::fingerprint::Fingerprint;
use pem::Pem;
use rand::{CryptoRng, Rng};
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use ring::test::rand::FixedSliceSequenceRandom;
use std::cell::UnsafeCell;
use std::time::{Duration, SystemTime};
use webrtc::dtls::crypto::{CryptoPrivateKey, CryptoPrivateKeyKind};
use webrtc::peer_connection::certificate::RTCCertificate;

#[derive(Clone, PartialEq)]
pub struct Certificate {
    inner: RTCCertificate,
}

/// The year 2000.
const UNIX_2000: i64 = 946645200;

/// The year 3000.
const UNIX_3000: i64 = 32503640400;

/// OID for the organisation name. See <http://oid-info.com/get/2.5.4.10>.
const ORGANISATION_NAME_OID: [u64; 4] = [2, 5, 4, 10];

/// OID for Elliptic Curve Public Key Cryptography. See <http://oid-info.com/get/1.2.840.10045.2.1>.
const EC_OID: [u64; 6] = [1, 2, 840, 10045, 2, 1];

/// OID for 256-bit Elliptic Curve Cryptography (ECC) with the P256 curve. See <http://oid-info.com/get/1.2.840.10045.3.1.7>.
const P256_OID: [u64; 7] = [1, 2, 840, 10045, 3, 1, 7];

/// OID for the ECDSA signature algorithm with using SHA256 as the hash function. See <http://oid-info.com/get/1.2.840.10045.4.3.2>.
const ECDSA_SHA256_OID: [u64; 7] = [1, 2, 840, 10045, 4, 3, 2];

impl Certificate {
    /// Generate new certificate.
    ///
    /// This function is pure and will generate the same certificate provided the exact same randomness source.
    pub fn generate<R>(rng: &mut R) -> Result<Self, Error>
    where
        R: CryptoRng + Rng,
    {
        // Usage of `FixedSliceSequenceRandom` guarantees us that we only use each byte-slice of pre-generated randomness once.
        let ring_rng = FixedSliceSequenceRandom {
            bytes: &[&rng.gen::<[u8; 32]>(), &rng.gen::<[u8; 32]>()],
            current: UnsafeCell::new(0),
        };

        let document = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &ring_rng)
            .map_err(Kind::Unspecified)?;
        let key_pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, document.as_ref())
                .map_err(Kind::KeyRejected)?;

        // Create a minimal certificate.
        let x509 = simple_x509::X509::builder()
            .issuer_utf8(Vec::from(ORGANISATION_NAME_OID), "rust-libp2p")
            .subject_utf8(Vec::from(ORGANISATION_NAME_OID), "rust-libp2p")
            .not_before_gen(UNIX_2000)
            .not_after_gen(UNIX_3000)
            .pub_key_ec(
                Vec::from(EC_OID),
                key_pair.public_key().as_ref().to_owned(),
                Vec::from(P256_OID),
            )
            .sign_oid(Vec::from(ECDSA_SHA256_OID))
            .build()
            .sign(
                |data, _| Some(key_pair.sign(&ring_rng, &data).ok()?.as_ref().to_owned()),
                &vec![], // We close over the keypair so no need to pass it.
            )
            .ok_or(Kind::FailedToSign)?;

        let der_encoded = x509.x509_enc().ok_or(Kind::FailedToEncode)?;

        let certificate = webrtc::dtls::crypto::Certificate {
            certificate: vec![rustls::Certificate(der_encoded.clone())],
            private_key: CryptoPrivateKey {
                kind: CryptoPrivateKeyKind::Ecdsa256(key_pair),
                serialized_der: document.as_ref().to_owned(),
            },
        };

        let certificate = RTCCertificate::from_existing(
            certificate,
            &pem::encode(&Pem {
                tag: "CERTIFICATE".to_string(),
                contents: der_encoded,
            }),
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(UNIX_3000 as u64))
                .expect("expiry to be always valid"),
        );

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
    KeyRejected(ring::error::KeyRejected),
    #[error(transparent)]
    Unspecified(ring::error::Unspecified),
    #[error("Failed to sign certificate")]
    FailedToSign,
    #[error("Failed to DER-encode certificate")]
    FailedToEncode,
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
            "-----BEGIN CERTIFICATE-----\r\nMIIBEDCBvgIBADAKBggqhkjOPQQDAjAWMRQwEgYDVQQKDAtydXN0LWxpYnAycDAi\r\nGA8xOTk5MTIzMTEzMDAwMFoYDzI5OTkxMjMxMTMwMDAwWjAWMRQwEgYDVQQKDAty\r\ndXN0LWxpYnAycDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABFSl0Byu8vlC6KNu\r\nTeN207dHFN6tMhJECnd+o/3Dr47JcOGdG5IUNXvdQDqhbQ9KcO9CdtgUy5BF7xlR\r\n9lhOdZQwCgYIKoZIzj0EAwIDQQBdpiGC3yxU6g+91aWxEfBPZF9WjQXRjysDUmzf\r\n5C4H1Ql1XNPpojRHPOrFnjYZwvolt2PxgFKEHYhZAIJBUyfy\r\n-----END CERTIFICATE-----\r\n"
        )
    }

    #[test]
    fn cloned_certificate_is_equivalent() {
        let certificate = Certificate::generate(&mut thread_rng()).unwrap();

        let cloned_certificate = certificate.clone();

        assert!(certificate == cloned_certificate)
    }
}
