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

//! Certificate handling for libp2p
//!
//! This module handles generation, signing, and verification of certificates.

use libp2p_core::identity;
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

/// Generates a self-signed TLS certificate that includes a libp2p-specific
/// certificate extension containing the public key of the given keypair.
pub fn make_certificate(
    keypair: &identity::Keypair,
) -> Result<rcgen::Certificate, rcgen::RcgenError> {

    // Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
    // Similarly, hash functions with an output length less than 256 bits MUST NOT be used.
    let signature_algorithm = &rcgen::PKCS_ECDSA_P256_SHA256;

    // The key used to generate and sign this certificate
    // SHOULD NOT be related to the host's key.
    // Endpoints MAY generate a new key and certificate
    // for every connection attempt, or they MAY reuse the same key
    // and certificate for multiple connections.
    let certif_keypair = rcgen::KeyPair::generate(signature_algorithm)?;

    // The certificate MUST contain the libp2p Public Key Extension.
    let libp2p_extension: rcgen::CustomExtension = {

        // The peer signs the concatenation of the string `libp2p-tls-handshake:`
        // and the public key that it used to generate the certificate carrying
        // the libp2p Public Key Extension, using its private host key.
        let signature = {
            let mut msg = vec![];
            msg.extend(P2P_SIGNING_PREFIX);
            msg.extend(certif_keypair.public_key_der());

            keypair.sign(&msg).map_err(|_| rcgen::RcgenError::RingUnspecified)?
        };

        // The public host key and the signature are ANS.1-encoded
        // into the SignedKey data structure, which is carried
        // in the libp2p Public Key Extension.
        // SignedKey ::= SEQUENCE {
        //    publicKey BIT STRING,
        //    signature BIT STRING
        // }
        let extension_content = {
            let serialized_pubkey = keypair.public().into_protobuf_encoding();
            yasna::encode_der(&(serialized_pubkey, signature))
        };

        // This extension MAY be marked critical.
        rcgen::CustomExtension::from_oid_content(&P2P_EXT_OID, extension_content)
    };

    let certificate = {
        let mut params = rcgen::CertificateParams::new(vec![]);
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.custom_extensions.push(libp2p_extension);
        params.alg = signature_algorithm;
        params.key_pair = Some(certif_keypair);
        rcgen::Certificate::from_params(params)?
    };

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

/// An x.509 certificate with a libp2p-specific extension
/// is used to secure libp2p connections.
pub struct P2pCertificate<'a> {
    certificate: X509Certificate<'a>,
    /// This is a specific libp2p Public Key Extension with two values:
    /// * the public host key
    /// * a signature performed using the private host key
    extension: P2pExtension,
}

/// Parse TLS certificate from DER input that includes a libp2p-specific
/// certificate extension containing the public key of the given keypair.
pub fn parse_certificate(der_input: &[u8]) -> Result<P2pCertificate, X509Error> {
    let x509 = X509Certificate::from_der(der_input)
        .map(|(_rest_input, x509)| x509)
        .map_err(|e| {
            use x509_parser::nom::Err::*;
            match e {
                Error(e) => e,
                Failure(e) => e,
                Incomplete(_) => X509Error::InvalidCertificate, // should never happen
            }
        })?;

    let p2p_ext_oid = der_parser::oid::Oid::from(&P2P_EXT_OID)
        .expect("This is a valid OID of p2p extension; qed");

    let mut libp2p_extension = None;

    for (oid, ext) in x509.extensions() {
        if oid == &p2p_ext_oid && libp2p_extension.is_some() {
            // The extension was already parsed
            return Err(X509Error::DuplicateExtensions)
        }

        if oid == &p2p_ext_oid {
            // The public host key and the signature are ANS.1-encoded
            // into the SignedKey data structure, which is carried
            // in the libp2p Public Key Extension.
            // SignedKey ::= SEQUENCE {
            //    publicKey BIT STRING,
            //    signature BIT STRING
            // }
            let (public_key, signature): (Vec<u8>, Vec<u8>) = yasna::decode_der(ext.value)
                .map_err(|_| X509Error::InvalidUserCertificate)?;
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
                .map_err(|_| X509Error::InvalidUserCertificate)?;
            let ext = P2pExtension { public_key, signature};
            libp2p_extension = Some(ext);
            continue;
        }

        if ext.critical {
            // Endpoints MUST abort the connection attempt if the certificate
            // contains critical extensions that the endpoint does not understand.
            return Err(X509Error::InvalidExtensions)
        }

        // Implementations MUST ignore non-critical extensions with unknown OIDs.
    }

    if let Some(extension) = libp2p_extension {
        Ok(P2pCertificate{
            certificate: x509,
            extension,
        })
    } else {
        // The certificate MUST contain the libp2p Public Key Extension.
        // If this extension is missing, endpoints MUST abort the connection attempt.
        Err(X509Error::InvalidCertificate)
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
    pub fn is_valid(&self) -> bool {

        // The certificate MUST have NotBefore and NotAfter fields set
        // such that the certificate is valid at the time it is received by the peer.
        if !self.certificate.validity().is_valid() {
            return false
        }

        // Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
        // Similarly, hash functions with an output length less than 256 bits
        // MUST NOT be used, due to the possibility of collision attacks.
        // In particular, MD5 and SHA1 MUST NOT be used.
        if let Some(signature_scheme) = self.signature_scheme() {
            // Endpoints MUST abort the connection attempt if the certificate’s
            // self-signature is not valid.
            let raw_certificate = self.certificate.tbs_certificate.as_ref();
            let signature = self.certificate.signature_value.data;
            // check if self signed
            if !self.verify_signature(signature_scheme, raw_certificate, signature) {
                return false
            }
        } else {
            // Endpoints MUST abort the connection attempt if it is not used.
            return false
        }

        // Dump PKI in the DER format:
        // SubjectPublicKeyInfo ::= SEQUENCE {
        //    algorithm             AlgorithmIdentifier,
        //    subject_public_key    BIT STRING
        // }
        // AlgorithmIdentifier  ::=  SEQUENCE  {
        //    algorithm               OBJECT IDENTIFIER,
        //    parameters              ANY DEFINED BY algorithm OPTIONAL
        // }
        let pki = &self.certificate.tbs_certificate.subject_pki;
        let algo_oid = if let Some(algo_oid) = pki.algorithm.algorithm.iter() {
            yasna::models::ObjectIdentifier::new(algo_oid.collect())
        } else {
            return false;
        };
        let params_der = match pki.algorithm.parameters.as_ref().map(der_parser::ber::BerObject::to_vec) {
            Some(Ok(der)) => Some(der),
            Some(Err(_)) => return false,
            _ => None
        };
        let subject_pki = yasna::construct_der(|writer| {
            writer.write_sequence(|writer| {
                writer.next().write_sequence(|writer| {
                    writer.next().write_oid(&algo_oid);
                    if let Some(params_der) = params_der {
                        writer.next().write_der(&params_der);
                    }
                });
                writer.next().write_bitvec_bytes(&pki.subject_public_key.data, pki.subject_public_key.data.len() * 8);
            })
        });

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
        self.extension.public_key.verify(&msg, &self.extension.signature)
    }

    /// Return the signature scheme corresponding to [`AlgorithmIdentifier`]s
    /// of `subject_pki` and `signature_algorithm`
    /// according to https://tools.ietf.org/id/draft-ietf-tls-tls13-21.html#rfc.section.4.2.3.
    pub fn signature_scheme(&self) -> Option<rustls::SignatureScheme> {
        // Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
        // Endpoints MUST abort the connection attempt if it is not used.
        use rustls::SignatureScheme::*;
        use oid_registry::*;
        let signature_algorithm = &self.certificate.signature_algorithm;
        let pki_algorithm = &self.certificate.tbs_certificate.subject_pki.algorithm;

        if pki_algorithm.algorithm == OID_PKCS1_RSAENCRYPTION {
            if signature_algorithm.algorithm == OID_PKCS1_SHA256WITHRSA {
                return Some(RSA_PKCS1_SHA256)
            }
            if signature_algorithm.algorithm == OID_PKCS1_SHA384WITHRSA {
                return Some(RSA_PKCS1_SHA384)
            }
            if signature_algorithm.algorithm == OID_PKCS1_SHA512WITHRSA {
                return Some(RSA_PKCS1_SHA512)
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
                let params = signature_algorithm.parameters.as_ref()?;
                let params = params.as_sequence().ok()?;
                let first_param = params.get(0)?;
                let hash_oid_der = first_param.as_slice().ok()?;
                let (_, obj) = der_parser::parse_der(hash_oid_der).ok()?;
                let hash_oid = obj.as_sequence().ok()?.get(0)?.as_oid_val().ok()?;

                if hash_oid == OID_NIST_HASH_SHA256 {
                    return Some(RSA_PSS_SHA256)
                }
                if hash_oid == OID_NIST_HASH_SHA384 {
                    return Some(RSA_PSS_SHA384)
                }
                if hash_oid == OID_NIST_HASH_SHA512 {
                    return Some(RSA_PSS_SHA512)
                }

                // Default hash algo is SHA-1, however:
                // In particular, MD5 and SHA1 MUST NOT be used.
                return None
            }
        }

        if pki_algorithm.algorithm == OID_KEY_TYPE_EC_PUBLIC_KEY {
            let signature_param = pki_algorithm.parameters.as_ref()?.as_oid_val().ok()?;
            if signature_param == OID_EC_P256 && signature_algorithm.algorithm == OID_SIG_ECDSA_WITH_SHA256 {
                return Some(ECDSA_NISTP256_SHA256)
            }
            if signature_param == OID_NIST_EC_P384 && signature_algorithm.algorithm == OID_SIG_ECDSA_WITH_SHA384 {
                return Some(ECDSA_NISTP384_SHA384)
            }
            if signature_param == OID_NIST_EC_P521 && signature_algorithm.algorithm == OID_SIG_ECDSA_WITH_SHA512 {
                return Some(ECDSA_NISTP521_SHA512)
            }
            return None
        }

        if signature_algorithm.algorithm == OID_SIG_ED25519 {
            return Some(ED25519)
        }
        if signature_algorithm.algorithm == OID_SIG_ED448 {
           return Some(ED448)
        }

        None
    }

    /// Get a [`ring::signature::UnparsedPublicKey`] for this `signature_scheme`.
    /// Return `None` if the `signature_scheme` does not match the public key signature
    /// and hashing algorithm.
    pub fn public_key(&self, signature_scheme: rustls::SignatureScheme) -> Option<ring::signature::UnparsedPublicKey<&[u8]>> {
        use rustls::SignatureScheme::*;
        use ring::signature;

        let current_signature_scheme = self.signature_scheme()?;
        if signature_scheme != current_signature_scheme {
            // This certificate was signed with a different signature scheme
            return None
        }

        let verification_algorithm: &dyn signature::VerificationAlgorithm = match signature_scheme {
            RSA_PKCS1_SHA256 => &signature::RSA_PKCS1_2048_8192_SHA256,
            RSA_PKCS1_SHA384 => &signature::RSA_PKCS1_2048_8192_SHA384,
            RSA_PKCS1_SHA512 => &signature::RSA_PKCS1_2048_8192_SHA512,
            ECDSA_NISTP256_SHA256 => &signature::ECDSA_P256_SHA256_ASN1,
            ECDSA_NISTP384_SHA384 => &signature::ECDSA_P384_SHA384_ASN1,
            ECDSA_NISTP521_SHA512 => {
                // See https://github.com/briansmith/ring/issues/824
                return None
            },
            RSA_PSS_SHA256 => &signature::RSA_PSS_2048_8192_SHA256,
            RSA_PSS_SHA384 => &signature::RSA_PSS_2048_8192_SHA384,
            RSA_PSS_SHA512 => &signature::RSA_PSS_2048_8192_SHA512,
            ED25519 => &signature::ED25519,
            ED448 => {
                // See https://github.com/briansmith/ring/issues/463
                return None
            },
            // Similarly, hash functions with an output length less than 256 bits
            // MUST NOT be used, due to the possibility of collision attacks.
            // In particular, MD5 and SHA1 MUST NOT be used.
            RSA_PKCS1_SHA1 => return None,
            ECDSA_SHA1_Legacy => return None,
            Unknown(_) => return None,
        };
        let spki = &self.certificate.tbs_certificate.subject_pki;
        let key = signature::UnparsedPublicKey::new(verification_algorithm, spki.subject_public_key.data);
        Some(key)
    }
    /// Verify the `signature` of the `message` signed by the private key corresponding to the public key stored
    /// in the certificate.
    pub fn verify_signature(
        &self, signature_scheme: rustls::SignatureScheme, message: &[u8], signature: &[u8],
    ) -> bool {
        if let Some(pk) = self.public_key(signature_scheme) {
            pk.verify(message, signature).is_ok()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::certificate;
    #[test]
    fn sanity_check() {
        let keypair = libp2p_core::identity::Keypair::generate_ed25519();
        let cert = certificate::make_certificate(&keypair).unwrap();
        let cert_der = cert.serialize_der().unwrap();
        let parsed_cert = certificate::parse_certificate(&cert_der).unwrap();
        assert!(parsed_cert.is_valid());
        assert_eq!(keypair.public(), parsed_cert.extension.public_key);
    }
}
