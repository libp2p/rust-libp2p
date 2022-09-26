// Copyright 2021 Parity Technologies (UK) Ltd.
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

//! X.509 certificate handling for libp2p
//!
//! This module handles generation, signing, and verification of certificates.

use libp2p_core::{identity, PeerId};
use x509_parser::prelude::*;

/// The libp2p Public Key Extension is a X.509 extension
/// with the Object Identier 1.3.6.1.4.1.53594.1.1,
/// allocated by IANA to the libp2p project at Protocol Labs.
const P2P_EXT_OID: [u64; 9] = [1, 3, 6, 1, 4, 1, 53594, 1, 1];

/// The peer signs the concatenation of the string `libp2p-tls-handshake:`
/// and the public key that it used to generate the certificate carrying
/// the libp2p Public Key Extension, using its private host key.
/// This signature provides cryptographic proof that the peer was
/// in possession of the private host key at the time the certificate was signed.
const P2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";

// Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
// Similarly, hash functions with an output length less than 256 bits MUST NOT be used.
static P2P_SIGNATURE_ALGORITHM: &rcgen::SignatureAlgorithm = &rcgen::PKCS_ECDSA_P256_SHA256;

/// Generates a self-signed TLS certificate that includes a libp2p-specific
/// certificate extension containing the public key of the given keypair.
pub fn generate(keypair: &identity::Keypair) -> Result<rcgen::Certificate, rcgen::RcgenError> {
    // Keypair used to sign the certificate.
    // SHOULD NOT be related to the host's key.
    // Endpoints MAY generate a new key and certificate
    // for every connection attempt, or they MAY reuse the same key
    // and certificate for multiple connections.
    let certif_keypair = rcgen::KeyPair::generate(P2P_SIGNATURE_ALGORITHM)?;

    // Generate the libp2p-specific extension.
    // The certificate MUST contain the libp2p Public Key Extension.
    let libp2p_extension: rcgen::CustomExtension = {
        // The peer signs the concatenation of the string `libp2p-tls-handshake:`
        // and the public key that it used to generate the certificate carrying
        // the libp2p Public Key Extension, using its private host key.
        let signature = {
            let mut msg = vec![];
            msg.extend(P2P_SIGNING_PREFIX);
            msg.extend(certif_keypair.public_key_der());

            keypair
                .sign(&msg)
                .map_err(|_| rcgen::RcgenError::RingUnspecified)?
        };

        // The public host key and the signature are ANS.1-encoded
        // into the SignedKey data structure, which is carried
        // in the libp2p Public Key Extension.
        // SignedKey ::= SEQUENCE {
        //    publicKey OCTET STRING,
        //    signature OCTET STRING
        // }
        let extension_content = {
            let serialized_pubkey = keypair.public().to_protobuf_encoding();
            yasna::encode_der(&(serialized_pubkey, signature))
        };

        // This extension MAY be marked critical.
        let mut ext = rcgen::CustomExtension::from_oid_content(&P2P_EXT_OID, extension_content);
        ext.set_criticality(true);
        ext
    };

    let certificate = {
        let mut params = rcgen::CertificateParams::new(vec![]);
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.custom_extensions.push(libp2p_extension);
        params.alg = P2P_SIGNATURE_ALGORITHM;
        params.key_pair = Some(certif_keypair);
        rcgen::Certificate::from_params(params)?
    };

    Ok(certificate)
}

/// Attempts to parse the provided bytes as a [`P2pCertificate`].
///
/// For this to succeed, the certificate must contain the specified extension and the signature must
/// match the embedded public key.
pub fn parse(der_input: &[u8]) -> Result<P2pCertificate, webpki::Error> {
    let x509 = X509Certificate::from_der(der_input)
        .map(|(_rest_input, x509)| x509)
        .map_err(|_| webpki::Error::BadDer)?;

    let p2p_ext_oid = der_parser::oid::Oid::from(&P2P_EXT_OID)
        .expect("This is a valid OID of p2p extension; qed");

    let mut libp2p_extension = None;

    for ext in x509.extensions() {
        let oid = &ext.oid;
        if oid == &p2p_ext_oid && libp2p_extension.is_some() {
            // The extension was already parsed
            return Err(webpki::Error::BadDer);
        }

        if oid == &p2p_ext_oid {
            // The public host key and the signature are ANS.1-encoded
            // into the SignedKey data structure, which is carried
            // in the libp2p Public Key Extension.
            // SignedKey ::= SEQUENCE {
            //    publicKey OCTET STRING,
            //    signature OCTET STRING
            // }
            let (public_key, signature): (Vec<u8>, Vec<u8>) =
                yasna::decode_der(ext.value).map_err(|_| webpki::Error::ExtensionValueInvalid)?;
            // The publicKey field of SignedKey contains the public host key
            // of the endpoint, encoded using the following protobuf:
            // enum KeyType {
            //    RSA = 0;
            //    Ed25519 = 1;
            //    Secp256k1 = 2;
            //    ECDSA = 3;
            // }
            // message PublicKey {
            //    required KeyType Type = 1;
            //    required bytes Data = 2;
            // }
            let public_key = identity::PublicKey::from_protobuf_encoding(&public_key)
                .map_err(|_| webpki::Error::UnknownIssuer)?;
            let ext = P2pExtension {
                public_key,
                signature,
            };
            libp2p_extension = Some(ext);
            continue;
        }

        if ext.critical {
            // Endpoints MUST abort the connection attempt if the certificate
            // contains critical extensions that the endpoint does not understand.
            return Err(webpki::Error::UnsupportedCriticalExtension);
        }

        // Implementations MUST ignore non-critical extensions with unknown OIDs.
    }

    // The certificate MUST contain the libp2p Public Key Extension.
    // If this extension is missing, endpoints MUST abort the connection attempt.
    let extension = libp2p_extension.ok_or(webpki::Error::BadDer)?;

    let certificate = P2pCertificate {
        certificate: x509,
        extension,
    };

    certificate.verify()?;

    Ok(certificate)
}

/// The contents of the specific libp2p extension, containing the public host key
/// and a signature performed using the private host key.
pub struct P2pExtension {
    public_key: identity::PublicKey,
    /// This signature provides cryptographic proof that the peer was
    /// in possession of the private host key at the time the certificate was signed.
    signature: Vec<u8>,
}

/// An X.509 certificate with a libp2p-specific extension
/// is used to secure libp2p connections.
pub struct P2pCertificate<'a> {
    certificate: X509Certificate<'a>,
    /// This is a specific libp2p Public Key Extension with two values:
    /// * the public host key
    /// * a signature performed using the private host key
    extension: P2pExtension,
}

impl<'a> P2pCertificate<'a> {
    /// The [`PeerId`] of the remote peer.
    pub fn peer_id(&self) -> PeerId {
        self.extension.public_key.to_peer_id()
    }
}

impl P2pCertificate<'_> {
    /// This method validates the certificate according to libp2p TLS 1.3 specs.
    /// The certificate MUST:
    /// 1. be valid at the time it is received by the peer;
    /// 2. use the NamedCurve encoding;
    /// 3. use hash functions with an output length not less than 256 bits;
    /// 4. be self signed;
    /// 5. contain a valid signature in the specific libp2p extension.
    fn verify(&self) -> Result<(), webpki::Error> {
        use webpki::Error;
        // The certificate MUST have NotBefore and NotAfter fields set
        // such that the certificate is valid at the time it is received by the peer.
        if !self.certificate.validity().is_valid() {
            return Err(Error::InvalidCertValidity);
        }

        // Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
        // Similarly, hash functions with an output length less than 256 bits
        // MUST NOT be used, due to the possibility of collision attacks.
        // In particular, MD5 and SHA1 MUST NOT be used.
        // Endpoints MUST abort the connection attempt if it is not used.
        let signature_scheme = self.signature_scheme()?;
        // Endpoints MUST abort the connection attempt if the certificate’s
        // self-signature is not valid.
        let raw_certificate = self.certificate.tbs_certificate.as_ref();
        let signature = self.certificate.signature_value.data;
        // check if self signed
        self.verify_signature(signature_scheme, raw_certificate, signature)
            .map_err(|_| Error::SignatureAlgorithmMismatch)?;

        let subject_pki = self.certificate.public_key().raw;

        // The peer signs the concatenation of the string `libp2p-tls-handshake:`
        // and the public key that it used to generate the certificate carrying
        // the libp2p Public Key Extension, using its private host key.
        let mut msg = vec![];
        msg.extend(P2P_SIGNING_PREFIX);
        msg.extend(subject_pki);

        // This signature provides cryptographic proof that the peer was in possession
        // of the private host key at the time the certificate was signed.
        // Peers MUST verify the signature, and abort the connection attempt
        // if signature verification fails.
        let user_owns_sk = self
            .extension
            .public_key
            .verify(&msg, &self.extension.signature);
        if !user_owns_sk {
            return Err(Error::UnknownIssuer);
        }

        Ok(())
    }

    /// Return the signature scheme corresponding to [`AlgorithmIdentifier`]s
    /// of `subject_pki` and `signature_algorithm`
    /// according to `<https://tools.ietf.org/id/draft-ietf-tls-tls13-21.html#rfc.section.4.2.3>`.
    pub fn signature_scheme(&self) -> Result<rustls::SignatureScheme, webpki::Error> {
        // Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
        // Endpoints MUST abort the connection attempt if it is not used.
        use oid_registry::*;
        use rustls::SignatureScheme::*;

        let signature_algorithm = &self.certificate.signature_algorithm;
        let pki_algorithm = &self.certificate.tbs_certificate.subject_pki.algorithm;

        if pki_algorithm.algorithm == OID_PKCS1_RSAENCRYPTION {
            if signature_algorithm.algorithm == OID_PKCS1_SHA256WITHRSA {
                return Ok(RSA_PKCS1_SHA256);
            }
            if signature_algorithm.algorithm == OID_PKCS1_SHA384WITHRSA {
                return Ok(RSA_PKCS1_SHA384);
            }
            if signature_algorithm.algorithm == OID_PKCS1_SHA512WITHRSA {
                return Ok(RSA_PKCS1_SHA512);
            }
            if signature_algorithm.algorithm == OID_PKCS1_RSASSAPSS {
                // According to https://datatracker.ietf.org/doc/html/rfc4055#section-3.1:
                // Inside of params there shuld be a sequence of:
                // - Hash Algorithm
                // - Mask Algorithm
                // - Salt Length
                // - Trailer Field

                // We are interested in Hash Algorithm only, however the der parser parses
                // params into a mess, so here is a workaround to fix it:
                fn get_hash_oid<'a>(
                    signature_algorithm: &'a AlgorithmIdentifier,
                ) -> Option<Oid<'a>> {
                    let params = signature_algorithm.parameters.as_ref()?;
                    let params = params.as_sequence().ok()?;
                    let first_param = params.get(0)?;
                    let hash_oid_der = first_param.as_slice().ok()?;
                    let (_, obj) = der_parser::parse_der(hash_oid_der).ok()?;
                    let hash_oid = obj.as_sequence().ok()?.get(0)?.as_oid_val().ok()?;
                    Some(hash_oid)
                }

                let hash_oid = get_hash_oid(signature_algorithm).ok_or(webpki::Error::BadDer)?;

                if hash_oid == OID_NIST_HASH_SHA256 {
                    return Ok(RSA_PSS_SHA256);
                }
                if hash_oid == OID_NIST_HASH_SHA384 {
                    return Ok(RSA_PSS_SHA384);
                }
                if hash_oid == OID_NIST_HASH_SHA512 {
                    return Ok(RSA_PSS_SHA512);
                }

                // Default hash algo is SHA-1, however:
                // In particular, MD5 and SHA1 MUST NOT be used.
                return Err(webpki::Error::UnsupportedSignatureAlgorithm);
            }
        }

        if pki_algorithm.algorithm == OID_KEY_TYPE_EC_PUBLIC_KEY {
            let signature_param = pki_algorithm
                .parameters
                .as_ref()
                .ok_or(webpki::Error::BadDer)?
                .as_oid_val()
                .map_err(|_| webpki::Error::BadDer)?;
            if signature_param == OID_EC_P256
                && signature_algorithm.algorithm == OID_SIG_ECDSA_WITH_SHA256
            {
                return Ok(ECDSA_NISTP256_SHA256);
            }
            if signature_param == OID_NIST_EC_P384
                && signature_algorithm.algorithm == OID_SIG_ECDSA_WITH_SHA384
            {
                return Ok(ECDSA_NISTP384_SHA384);
            }
            if signature_param == OID_NIST_EC_P521
                && signature_algorithm.algorithm == OID_SIG_ECDSA_WITH_SHA512
            {
                return Ok(ECDSA_NISTP521_SHA512);
            }
            return Err(webpki::Error::UnsupportedSignatureAlgorithm);
        }

        if signature_algorithm.algorithm == OID_SIG_ED25519 {
            return Ok(ED25519);
        }
        if signature_algorithm.algorithm == OID_SIG_ED448 {
            return Ok(ED448);
        }

        Err(webpki::Error::UnsupportedSignatureAlgorithm)
    }

    /// Get a [`ring::signature::UnparsedPublicKey`] for this `signature_scheme`.
    /// Return `Error` if the `signature_scheme` does not match the public key signature
    /// and hashing algorithm or if the `signature_scheme` is not supported.
    pub fn public_key(
        &self,
        signature_scheme: rustls::SignatureScheme,
    ) -> Result<ring::signature::UnparsedPublicKey<&[u8]>, webpki::Error> {
        use ring::signature;
        use rustls::SignatureScheme::*;

        let current_signature_scheme = self.signature_scheme()?;
        if signature_scheme != current_signature_scheme {
            // This certificate was signed with a different signature scheme
            return Err(webpki::Error::UnsupportedSignatureAlgorithmForPublicKey);
        }

        let verification_algorithm: &dyn signature::VerificationAlgorithm = match signature_scheme {
            RSA_PKCS1_SHA256 => &signature::RSA_PKCS1_2048_8192_SHA256,
            RSA_PKCS1_SHA384 => &signature::RSA_PKCS1_2048_8192_SHA384,
            RSA_PKCS1_SHA512 => &signature::RSA_PKCS1_2048_8192_SHA512,
            ECDSA_NISTP256_SHA256 => &signature::ECDSA_P256_SHA256_ASN1,
            ECDSA_NISTP384_SHA384 => &signature::ECDSA_P384_SHA384_ASN1,
            ECDSA_NISTP521_SHA512 => {
                // See https://github.com/briansmith/ring/issues/824
                return Err(webpki::Error::UnsupportedSignatureAlgorithm);
            }
            RSA_PSS_SHA256 => &signature::RSA_PSS_2048_8192_SHA256,
            RSA_PSS_SHA384 => &signature::RSA_PSS_2048_8192_SHA384,
            RSA_PSS_SHA512 => &signature::RSA_PSS_2048_8192_SHA512,
            ED25519 => &signature::ED25519,
            ED448 => {
                // See https://github.com/briansmith/ring/issues/463
                return Err(webpki::Error::UnsupportedSignatureAlgorithm);
            }
            // Similarly, hash functions with an output length less than 256 bits
            // MUST NOT be used, due to the possibility of collision attacks.
            // In particular, MD5 and SHA1 MUST NOT be used.
            RSA_PKCS1_SHA1 => return Err(webpki::Error::UnsupportedSignatureAlgorithm),
            ECDSA_SHA1_Legacy => return Err(webpki::Error::UnsupportedSignatureAlgorithm),
            Unknown(_) => return Err(webpki::Error::UnsupportedSignatureAlgorithm),
        };
        let spki = &self.certificate.tbs_certificate.subject_pki;
        let key =
            signature::UnparsedPublicKey::new(verification_algorithm, spki.subject_public_key.data);

        Ok(key)
    }

    /// Verify the `signature` of the `message` signed by the private key corresponding to the public key stored
    /// in the certificate.
    pub fn verify_signature(
        &self,
        signature_scheme: rustls::SignatureScheme,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), webpki::Error> {
        let pk = self.public_key(signature_scheme)?;
        pk.verify(message, signature)
            .map_err(|_| webpki::Error::InvalidSignatureForPublicKey)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanity_check() {
        let keypair = identity::Keypair::generate_ed25519();
        let cert = generate(&keypair).unwrap();

        let cert_der = cert.serialize_der().unwrap();
        let parsed_cert = parse(&cert_der).unwrap();

        assert!(parsed_cert.verify().is_ok());
        assert_eq!(keypair.public(), parsed_cert.extension.public_key);
    }

    macro_rules! check_cert {
        ($name:ident, $path:literal, $scheme:path) => {
            #[test]
            fn $name() {
                let cert: &[u8] = include_bytes!($path);

                let cert = parse_certificate(cert).unwrap();
                assert!(cert.verify().is_err()); // Because p2p extension
                                                 // was not signed with the private key
                                                 // of the certificate.
                assert_eq!(cert.signature_scheme(), Ok($scheme));
            }
        };
    }

    check_cert! {ed448, "./test_assets/ed448.der", rustls::SignatureScheme::ED448}
    check_cert! {ed25519, "./test_assets/ed25519.der", rustls::SignatureScheme::ED25519}
    check_cert! {rsa_pkcs1_sha256, "./test_assets/rsa_pkcs1_sha256.der", rustls::SignatureScheme::RSA_PKCS1_SHA256}
    check_cert! {rsa_pkcs1_sha384, "./test_assets/rsa_pkcs1_sha384.der", rustls::SignatureScheme::RSA_PKCS1_SHA384}
    check_cert! {rsa_pkcs1_sha512, "./test_assets/rsa_pkcs1_sha512.der", rustls::SignatureScheme::RSA_PKCS1_SHA512}
    check_cert! {nistp256_sha256, "./test_assets/nistp256_sha256.der", rustls::SignatureScheme::ECDSA_NISTP256_SHA256}
    check_cert! {nistp384_sha384, "./test_assets/nistp384_sha384.der", rustls::SignatureScheme::ECDSA_NISTP384_SHA384}
    check_cert! {nistp521_sha512, "./test_assets/nistp521_sha512.der", rustls::SignatureScheme::ECDSA_NISTP521_SHA512}

    #[test]
    fn rsa_pss_sha384() {
        let cert: &[u8] = include_bytes!("./test_assets/rsa_pss_sha384.der");

        let cert = parse(cert).unwrap();
        cert.verify().unwrap(); // that was a fairly generated certificate.

        assert_eq!(
            cert.signature_scheme(),
            Ok(rustls::SignatureScheme::RSA_PSS_SHA384)
        );
    }

    #[test]
    fn nistp384_sha256() {
        let cert: &[u8] = include_bytes!("./test_assets/nistp384_sha256.der");

        let cert = parse(cert).unwrap();

        assert!(cert.signature_scheme().is_err());
    }
}
