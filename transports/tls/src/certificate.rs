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

use libp2p_identity as identity;
use libp2p_identity::PeerId;
use x509_parser::{prelude::*, signature_algorithm::SignatureAlgorithm};

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
pub fn generate(
    identity_keypair: &identity::Keypair,
) -> Result<(rustls::Certificate, rustls::PrivateKey), GenError> {
    // Keypair used to sign the certificate.
    // SHOULD NOT be related to the host's key.
    // Endpoints MAY generate a new key and certificate
    // for every connection attempt, or they MAY reuse the same key
    // and certificate for multiple connections.
    let certificate_keypair = rcgen::KeyPair::generate(P2P_SIGNATURE_ALGORITHM)?;
    let rustls_key = rustls::PrivateKey(certificate_keypair.serialize_der());

    let certificate = {
        let mut params = rcgen::CertificateParams::new(vec![]);
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.custom_extensions.push(make_libp2p_extension(
            identity_keypair,
            &certificate_keypair,
        )?);
        params.alg = P2P_SIGNATURE_ALGORITHM;
        params.key_pair = Some(certificate_keypair);
        rcgen::Certificate::from_params(params)?
    };

    let rustls_certificate = rustls::Certificate(certificate.serialize_der()?);

    Ok((rustls_certificate, rustls_key))
}

/// Attempts to parse the provided bytes as a [`P2pCertificate`].
///
/// For this to succeed, the certificate must contain the specified extension and the signature must
/// match the embedded public key.
pub fn parse(certificate: &rustls::Certificate) -> Result<P2pCertificate<'_>, ParseError> {
    let certificate = parse_unverified(certificate.as_ref())?;

    certificate.verify()?;

    Ok(certificate)
}

/// An X.509 certificate with a libp2p-specific extension
/// is used to secure libp2p connections.
#[derive(Debug)]
pub struct P2pCertificate<'a> {
    certificate: X509Certificate<'a>,
    /// This is a specific libp2p Public Key Extension with two values:
    /// * the public host key
    /// * a signature performed using the private host key
    extension: P2pExtension,
}

/// The contents of the specific libp2p extension, containing the public host key
/// and a signature performed using the private host key.
#[derive(Debug)]
pub struct P2pExtension {
    public_key: identity::PublicKey,
    /// This signature provides cryptographic proof that the peer was
    /// in possession of the private host key at the time the certificate was signed.
    signature: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct GenError(#[from] rcgen::RcgenError);

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ParseError(#[from] pub(crate) webpki::Error);

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct VerificationError(#[from] pub(crate) webpki::Error);

/// Internal function that only parses but does not verify the certificate.
///
/// Useful for testing but unsuitable for production.
fn parse_unverified(der_input: &[u8]) -> Result<P2pCertificate, webpki::Error> {
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
            let public_key = identity::PublicKey::try_decode_protobuf(&public_key)
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

    Ok(certificate)
}

fn make_libp2p_extension(
    identity_keypair: &identity::Keypair,
    certificate_keypair: &rcgen::KeyPair,
) -> Result<rcgen::CustomExtension, rcgen::RcgenError> {
    // The peer signs the concatenation of the string `libp2p-tls-handshake:`
    // and the public key that it used to generate the certificate carrying
    // the libp2p Public Key Extension, using its private host key.
    let signature = {
        let mut msg = vec![];
        msg.extend(P2P_SIGNING_PREFIX);
        msg.extend(certificate_keypair.public_key_der());

        identity_keypair
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
        let serialized_pubkey = identity_keypair.public().encode_protobuf();
        yasna::encode_der(&(serialized_pubkey, signature))
    };

    // This extension MAY be marked critical.
    let mut ext = rcgen::CustomExtension::from_oid_content(&P2P_EXT_OID, extension_content);
    ext.set_criticality(true);

    Ok(ext)
}

impl P2pCertificate<'_> {
    /// The [`PeerId`] of the remote peer.
    pub fn peer_id(&self) -> PeerId {
        self.extension.public_key.to_peer_id()
    }

    /// Verify the `signature` of the `message` signed by the private key corresponding to the public key stored
    /// in the certificate.
    pub fn verify_signature(
        &self,
        signature_scheme: rustls::SignatureScheme,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), VerificationError> {
        let pk = self.public_key(signature_scheme)?;
        pk.verify(message, signature)
            .map_err(|_| webpki::Error::InvalidSignatureForPublicKey)?;

        Ok(())
    }

    /// Get a [`ring::signature::UnparsedPublicKey`] for this `signature_scheme`.
    /// Return `Error` if the `signature_scheme` does not match the public key signature
    /// and hashing algorithm or if the `signature_scheme` is not supported.
    fn public_key(
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
            _ => return Err(webpki::Error::UnsupportedSignatureAlgorithm),
        };
        let spki = &self.certificate.tbs_certificate.subject_pki;
        let key = signature::UnparsedPublicKey::new(
            verification_algorithm,
            spki.subject_public_key.as_ref(),
        );

        Ok(key)
    }

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
        let signature = self.certificate.signature_value.as_ref();
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
    /// according to <https://www.rfc-editor.org/rfc/rfc8446.html#section-4.2.3>.
    fn signature_scheme(&self) -> Result<rustls::SignatureScheme, webpki::Error> {
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

                // We are interested in Hash Algorithm only

                if let Ok(SignatureAlgorithm::RSASSA_PSS(params)) =
                    SignatureAlgorithm::try_from(signature_algorithm)
                {
                    let hash_oid = params.hash_algorithm_oid();
                    if hash_oid == &OID_NIST_HASH_SHA256 {
                        return Ok(RSA_PSS_SHA256);
                    }
                    if hash_oid == &OID_NIST_HASH_SHA384 {
                        return Ok(RSA_PSS_SHA384);
                    }
                    if hash_oid == &OID_NIST_HASH_SHA512 {
                        return Ok(RSA_PSS_SHA512);
                    }
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
                .as_oid()
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn sanity_check() {
        let keypair = identity::Keypair::generate_ed25519();

        let (cert, _) = generate(&keypair).unwrap();
        let parsed_cert = parse(&cert).unwrap();

        assert!(parsed_cert.verify().is_ok());
        assert_eq!(keypair.public(), parsed_cert.extension.public_key);
    }

    macro_rules! check_cert {
        ($name:ident, $path:literal, $scheme:path) => {
            #[test]
            fn $name() {
                let cert: &[u8] = include_bytes!($path);

                let cert = parse_unverified(cert).unwrap();
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
        let cert = rustls::Certificate(include_bytes!("./test_assets/rsa_pss_sha384.der").to_vec());

        let cert = parse(&cert).unwrap();

        assert_eq!(
            cert.signature_scheme(),
            Ok(rustls::SignatureScheme::RSA_PSS_SHA384)
        );
    }

    #[test]
    fn nistp384_sha256() {
        let cert: &[u8] = include_bytes!("./test_assets/nistp384_sha256.der");

        let cert = parse_unverified(cert).unwrap();

        assert!(cert.signature_scheme().is_err());
    }

    #[test]
    fn can_parse_certificate_with_ed25519_keypair() {
        let certificate = rustls::Certificate(hex!("308201773082011ea003020102020900f5bd0debaa597f52300a06082a8648ce3d04030230003020170d3735303130313030303030305a180f34303936303130313030303030305a30003059301306072a8648ce3d020106082a8648ce3d030107034200046bf9871220d71dcb3483ecdfcbfcc7c103f8509d0974b3c18ab1f1be1302d643103a08f7a7722c1b247ba3876fe2c59e26526f479d7718a85202ddbe47562358a37f307d307b060a2b0601040183a25a01010101ff046a30680424080112207fda21856709c5ae12fd6e8450623f15f11955d384212b89f56e7e136d2e17280440aaa6bffabe91b6f30c35e3aa4f94b1188fed96b0ffdd393f4c58c1c047854120e674ce64c788406d1c2c4b116581fd7411b309881c3c7f20b46e54c7e6fe7f0f300a06082a8648ce3d040302034700304402207d1a1dbd2bda235ff2ec87daf006f9b04ba076a5a5530180cd9c2e8f6399e09d0220458527178c7e77024601dbb1b256593e9b96d961b96349d1f560114f61a87595").to_vec());

        let peer_id = parse(&certificate).unwrap().peer_id();

        assert_eq!(
            "12D3KooWJRSrypvnpHgc6ZAgyCni4KcSmbV7uGRaMw5LgMKT18fq"
                .parse::<PeerId>()
                .unwrap(),
            peer_id
        );
    }

    #[test]
    fn fails_to_parse_bad_certificate_with_ed25519_keypair() {
        let certificate = rustls::Certificate(hex!("308201773082011da003020102020830a73c5d896a1109300a06082a8648ce3d04030230003020170d3735303130313030303030305a180f34303936303130313030303030305a30003059301306072a8648ce3d020106082a8648ce3d03010703420004bbe62df9a7c1c46b7f1f21d556deec5382a36df146fb29c7f1240e60d7d5328570e3b71d99602b77a65c9b3655f62837f8d66b59f1763b8c9beba3be07778043a37f307d307b060a2b0601040183a25a01010101ff046a3068042408011220ec8094573afb9728088860864f7bcea2d4fd412fef09a8e2d24d482377c20db60440ecabae8354afa2f0af4b8d2ad871e865cb5a7c0c8d3dbdbf42de577f92461a0ebb0a28703e33581af7d2a4f2270fc37aec6261fcc95f8af08f3f4806581c730a300a06082a8648ce3d040302034800304502202dfb17a6fa0f94ee0e2e6a3b9fb6e986f311dee27392058016464bd130930a61022100ba4b937a11c8d3172b81e7cd04aedb79b978c4379c2b5b24d565dd5d67d3cb3c").to_vec());

        let error = parse(&certificate).unwrap_err();

        assert_eq!(format!("{error}"), "UnknownIssuer");
    }

    #[test]
    fn can_parse_certificate_with_ecdsa_keypair() {
        let certificate = rustls::Certificate(hex!("308201c030820166a003020102020900eaf419a6e3edb4a6300a06082a8648ce3d04030230003020170d3735303130313030303030305a180f34303936303130313030303030305a30003059301306072a8648ce3d020106082a8648ce3d030107034200048dbf1116c7c608d6d5292bd826c3feb53483a89fce434bf64538a359c8e07538ff71f6766239be6a146dcc1a5f3bb934bcd4ae2ae1d4da28ac68b4a20593f06ba381c63081c33081c0060a2b0601040183a25a01010101ff0481ae3081ab045f0803125b3059301306072a8648ce3d020106082a8648ce3d0301070342000484b93fa456a74bd0153919f036db7bc63c802f055bc7023395d0203de718ee0fc7b570b767cdd858aca6c7c4113ff002e78bd2138ac1a3b26dde3519e06979ad04483046022100bc84014cea5a41feabdf4c161096564b9ccf4b62fbef4fe1cd382c84e11101780221009204f086a84cb8ed8a9ddd7868dc90c792ee434adf62c66f99a08a5eba11615b300a06082a8648ce3d0403020348003045022054b437be9a2edf591312d68ff24bf91367ad4143f76cf80b5658f232ade820da022100e23b48de9df9c25d4c83ddddf75d2676f0b9318ee2a6c88a736d85eab94a912f").to_vec());

        let peer_id = parse(&certificate).unwrap().peer_id();

        assert_eq!(
            "QmZcrvr3r4S3QvwFdae3c2EWTfo792Y14UpzCZurhmiWeX"
                .parse::<PeerId>()
                .unwrap(),
            peer_id
        );
    }

    #[test]
    fn can_parse_certificate_with_secp256k1_keypair() {
        let certificate = rustls::Certificate(hex!("3082018230820128a003020102020900f3b305f55622cfdf300a06082a8648ce3d04030230003020170d3735303130313030303030305a180f34303936303130313030303030305a30003059301306072a8648ce3d020106082a8648ce3d0301070342000458f7e9581748ff9bdd933b655cc0e5552a1248f840658cc221dec2186b5a2fe4641b86ab7590a3422cdbb1000cf97662f27e5910d7569f22feed8829c8b52e0fa38188308185308182060a2b0601040183a25a01010101ff0471306f042508021221026b053094d1112bce799dc8026040ae6d4eb574157929f1598172061f753d9b1b04463044022040712707e97794c478d93989aaa28ae1f71c03af524a8a4bd2d98424948a782302207b61b7f074b696a25fb9e0059141a811cccc4cc28042d9301b9b2a4015e87470300a06082a8648ce3d04030203480030450220143ae4d86fdc8675d2480bb6912eca5e39165df7f572d836aa2f2d6acfab13f8022100831d1979a98f0c4a6fb5069ca374de92f1a1205c962a6d90ad3d7554cb7d9df4").to_vec());

        let peer_id = parse(&certificate).unwrap().peer_id();

        assert_eq!(
            "16Uiu2HAm2dSCBFxuge46aEt7U1oejtYuBUZXxASHqmcfVmk4gsbx"
                .parse::<PeerId>()
                .unwrap(),
            peer_id
        );
    }
}
