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

/// Generates a self-signed TLS certificate that includes a libp2p-specific
/// certificate extension containing the public key of the given keypair.
pub fn make_certificate(
    keypair: &identity::Keypair,
) -> Result<rcgen::Certificate, rcgen::RcgenError> {

    // Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
    // Similarly, hash functions with an output length less than 256 bits MUST NOT be used
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
            msg.extend(*b"libp2p-tls-handshake:");
            msg.extend(certif_keypair.public_key_der());

            keypair.sign(&msg).expect("Let's hope the signing won't fail")
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

        // The libp2p Public Key Extension is a X.509 extension
        // with the Object Identier 1.3.6.1.4.1.53594.1.1,
        // allocated by IANA to the libp2p project at Protocol Labs.
        let libp2p_ext_oid = &[1, 3, 6, 1, 4, 1, 53594, 1, 1];  // Based on libp2p TLS 1.3 specs

        // This extension MAY be marked critical.
        rcgen::CustomExtension::from_oid_content(libp2p_ext_oid, extension_content)
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

// The contents of the specific libp2p extension, containing the public host key
// and a signature performed using the private host key
pub struct P2pExtension {
    public_key: identity::PublicKey,
    signature: Vec<u8>,
}

pub struct P2pCertificate<'a> {
    certificate: X509Certificate<'a>,
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
                Incomplete(_) => X509Error::Generic, // should never happen
            }
        })?;

    let p2p_ext_oid = der_parser::oid::Oid::from(&[1, 3, 6, 1, 4, 1, 53594, 1, 1])
        .expect("This is a valid OID of p2p extension; qed");

    let mut libp2p_extension = None;

    for (oid, ext) in x509.extensions() {
        if oid == &p2p_ext_oid && libp2p_extension.is_some() {
            // The extension was already parsed
            return Err(X509Error::DuplicateExtensions)
        }

        if oid == &p2p_ext_oid {
            let (public_key, signature): (Vec<u8>, Vec<u8>) = yasna::decode_der(ext.value)
                .map_err(|_| X509Error::InvalidUserCertificate)?;
            let public_key = identity::PublicKey::from_protobuf_encoding(&public_key)
                .map_err(|_| X509Error::InvalidUserCertificate)?;
            let ext = P2pExtension { public_key, signature};
            libp2p_extension = Some(ext)
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
        // If this extension is missing, endpoints MUST abort the connection attempt.
        Err(X509Error::InvalidCertificate)
    }
}

impl P2pCertificate<'_> {
    pub fn is_valid(&self) -> bool {
        // Endpoints MUST abort the connection attempt if the certificate’s
        // self-signature is not valid.
        let self_signed = self.certificate.verify_signature(None).is_ok();
        if !self_signed {
            return false
        }

        // Certificates MUST use the NamedCurve encoding for elliptic curve parameters.
        // Endpoints MUST abort the connection attempt if is not used.
        let alg_oid = self.certificate.signature_algorithm.algorithm.iter();
        if alg_oid.is_none() {
            return false;
        }
        let alg_oid = alg_oid.expect("Could not be empty; qed");
        let signature_algorithm = rcgen::SignatureAlgorithm::from_oid(&alg_oid.collect::<Vec<_>>());
        if signature_algorithm.is_err() {
            return false;
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
        let subject_pki = yasna::construct_der(|writer| {
            writer.write_sequence(|writer| {
                writer.next().write_sequence(|writer| {
                    let oid = yasna::models::ObjectIdentifier::new(pki.algorithm.algorithm.iter().unwrap().collect());
                    writer.next().write_oid(&oid);
                    if let Some(params) = &pki.algorithm.parameters {
                        writer.next().write_der(&params.to_vec().unwrap());
                    }
                });
                writer.next().write_bitvec_bytes(&pki.subject_public_key.data, pki.subject_public_key.data.len() * 8);
            })
        });

        // The peer signs the concatenation of the string `libp2p-tls-handshake:`
        // and the public key that it used to generate the certificate carrying
        // the libp2p Public Key Extension, using its private host key.
        let mut msg = vec![];
        msg.extend(*b"libp2p-tls-handshake:");
        msg.extend(subject_pki);

        // This signature provides cryptographic proof that the peer was in possession
        // of the private host key at the time the certificate was signed.
        // Peers MUST verify the signature, and abort the connection attempt
        // if signature verification fails.
        self.extension.public_key.verify(&msg, &self.extension.signature)
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
