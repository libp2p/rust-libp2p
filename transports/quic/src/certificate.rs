// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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
//! This handles generation, signing, and verification.
//!
//! This crate uses the `log` crate to emit log output.  Events that will occur normally are output
//! at `trace` level, while “expected” error conditions (ones that can result during correct use of the
//! library) are logged at `debug` level.
use log::{debug, trace, warn};
use quinn_proto::Side;
static ALL_SUPPORTED_SIGNATURE_ALGORITHMS: &'static [&'static webpki::SignatureAlgorithm] = {
    &[
        &webpki::ECDSA_P256_SHA256,
        &webpki::ECDSA_P256_SHA384,
        &webpki::ECDSA_P384_SHA256,
        &webpki::ECDSA_P384_SHA384,
        &webpki::ED25519,
        &webpki::RSA_PKCS1_2048_8192_SHA256,
        &webpki::RSA_PKCS1_2048_8192_SHA384,
        &webpki::RSA_PKCS1_2048_8192_SHA512,
        &webpki::RSA_PKCS1_3072_8192_SHA384,
        &webpki::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
        &webpki::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
        &webpki::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
    ]
};
const BASIC_CONSTRAINTS_OID: &[u64] = &[2, 5, 29, 19];
const LIBP2P_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 53594, 1, 1];
const LIBP2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";
pub const LIBP2P_SIGNING_PREFIX_LENGTH: usize = LIBP2P_SIGNING_PREFIX.len();
const LIBP2P_SIGNATURE_ALGORITHM_PUBLIC_KEY_LENGTH: usize = 65;
static LIBP2P_SIGNATURE_ALGORITHM: &'static rcgen::SignatureAlgorithm =
    &rcgen::PKCS_ECDSA_P256_SHA256;
use libp2p_core::identity;

fn encode_signed_key(public_key: identity::PublicKey, signature: &[u8]) -> rcgen::CustomExtension {
    let public_key = public_key.into_protobuf_encoding();
    let contents = yasna::construct_der(|writer| {
        writer.write_sequence(|writer| {
            writer
                .next()
                .write_bitvec_bytes(&public_key, public_key.len() * 8);
            writer
                .next()
                .write_bitvec_bytes(signature, signature.len() * 8);
        })
    });
    rcgen::CustomExtension::from_oid_content(LIBP2P_OID, contents)
}

fn gen_signed_keypair(keypair: &identity::Keypair) -> (rcgen::KeyPair, rcgen::CustomExtension) {
    let temp_keypair = rcgen::KeyPair::generate(&LIBP2P_SIGNATURE_ALGORITHM)
        .expect("we pass valid parameters, and assume we have enough memory and randomness; qed");
    let mut signing_buf =
        [0u8; LIBP2P_SIGNING_PREFIX_LENGTH + LIBP2P_SIGNATURE_ALGORITHM_PUBLIC_KEY_LENGTH];
    let public = temp_keypair.public_key_raw();
    assert_eq!(
        public.len(),
        LIBP2P_SIGNATURE_ALGORITHM_PUBLIC_KEY_LENGTH,
        "ECDSA public keys are 65 bytes"
    );
    signing_buf[..LIBP2P_SIGNING_PREFIX_LENGTH].copy_from_slice(&LIBP2P_SIGNING_PREFIX[..]);
    signing_buf[LIBP2P_SIGNING_PREFIX_LENGTH..].copy_from_slice(public);
    let signature = keypair.sign(&signing_buf).expect("signing failed");
    (
        temp_keypair,
        encode_signed_key(keypair.public(), &signature),
    )
}

pub fn make_cert(keypair: &identity::Keypair) -> rcgen::Certificate {
    let mut params = rcgen::CertificateParams::new(vec![]);
    let (cert_keypair, libp2p_extension) = gen_signed_keypair(keypair);
    params.custom_extensions.push(libp2p_extension);
    params.alg = &LIBP2P_SIGNATURE_ALGORITHM;
    params.key_pair = Some(cert_keypair);
    rcgen::Certificate::from_params(params)
        .expect("certificate generation with valid params will succeed; qed")
}

/// Read a bitvec into a vector of bytes.  Requires the bitvec to be a whole number of bytes.
fn read_bitvec(reader: &mut yasna::BERReaderSeq) -> Result<Vec<u8>, yasna::ASN1Error> {
    let (value, bits) = reader.next().read_bitvec_bytes()?;
    // be extra careful regarding overflow
    if (bits & 7) == 0 && value.len() == (bits >> 3) {
        Ok(value)
    } else {
        warn!("value was of wrong length, sorry!");
        Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))
    }
}

/// Read an X.509 Algorithm Identifier
fn read_algid(reader: &mut yasna::BERReaderSeq) -> Result<AlgorithmIdentifier, yasna::ASN1Error> {
    reader.next().read_sequence(|reader| {
        let algorithm = reader.next().read_oid()?;
        let parameters = reader.read_optional(|reader| reader.read_der())?;
        Ok(AlgorithmIdentifier {
            algorithm,
            parameters,
        })
    })
}

#[derive(PartialEq, Eq, Debug)]
struct AlgorithmIdentifier {
    algorithm: yasna::models::ObjectIdentifier,
    parameters: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct X509Certificate {
    tbs_certificate: Vec<u8>,
    algorithm: AlgorithmIdentifier,
    signature_value: Vec<u8>,
}

fn parse_x509_extensions(
    reader: &mut yasna::BERReaderSeq,
    certificate_key: &[u8],
) -> Result<identity::PublicKey, yasna::ASN1Error> {
    reader.next().read_tagged(yasna::Tag::context(3), |reader| {
        let mut public_key = None;
        let mut oids_seen = std::collections::HashSet::new();
        reader.read_sequence_of(|reader| {
            trace!("reading an extension");
            Ok(public_key = parse_x509_extension(reader, &certificate_key, &mut oids_seen)?)
        })?;
        match public_key {
            Some(e) => Ok(e),
            None => Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Eof)),
        }
    })
}

fn parse_x509_extension(
    reader: yasna::BERReader,
    certificate_key: &[u8],
    oids_seen: &mut std::collections::HashSet<yasna::models::ObjectIdentifier>,
) -> Result<Option<identity::PublicKey>, yasna::ASN1Error> {
    reader.read_sequence(|reader| {
        let oid = reader.next().read_oid()?;
        trace!("read extensions with oid {:?}", oid);
        if !oids_seen.insert(oid.clone()) {
            warn!(
                "found the second extension with oid {:?} in the same certificate",
                oid
            );
            return Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid));
        }
        let mut is_critical = false;
        reader.read_optional(|reader| {
            if reader.read_bool()? {
                Ok(is_critical = true)
            } else {
                Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))
            }
        })?;
        trace!("certificate critical? {}", is_critical);
        let contents = reader.next().read_bytes()?;
        match &**oid.components() {
            LIBP2P_OID => Ok(Some(verify_libp2p_extension(&contents, certificate_key)?)),
            BASIC_CONSTRAINTS_OID => yasna::parse_der(&contents, |reader| {
                reader.read_sequence(|reader| reader.read_optional(|reader| reader.read_bool()))
            })
            .map(|_| None),
            _ => {
                if is_critical {
                    // unknown critical extension
                    Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))
                } else {
                    Ok(None)
                }
            }
        }
    })
}

fn verify_libp2p_extension(
    extension: &[u8],
    certificate_key: &[u8],
) -> yasna::ASN1Result<identity::PublicKey> {
    yasna::parse_der(extension, |reader| {
        reader.read_sequence(|mut reader| {
            let public_key = read_bitvec(&mut reader)?;
            let signature = read_bitvec(&mut reader)?;
            let public_key = identity::PublicKey::from_protobuf_encoding(&public_key)
                .map_err(|_| yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))?;
            let mut v = Vec::with_capacity(LIBP2P_SIGNING_PREFIX_LENGTH + certificate_key.len());
            v.extend_from_slice(&LIBP2P_SIGNING_PREFIX[..]);
            v.extend_from_slice(certificate_key);
            if public_key.verify(&v, &signature) {
                Ok(public_key)
            } else {
                Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))
            }
        })
    })
}

fn parse_certificate(certificate: &[u8]) -> yasna::ASN1Result<identity::PublicKey> {
    trace!("parsing certificate");
    let raw_certificate = yasna::parse_der(certificate, |reader| {
        reader.read_sequence(|mut reader| {
            let tbs_certificate = reader.next().read_der()?;
            let algorithm = read_algid(&mut reader)?;
            let signature_value = read_bitvec(&mut reader)?;
            Ok(X509Certificate {
                tbs_certificate,
                algorithm,
                signature_value,
            })
        })
    })?;
    yasna::parse_der(&raw_certificate.tbs_certificate, |reader| {
        trace!("parsing TBScertificate");
        reader.read_sequence(|mut reader| {
            trace!("getting X509 version");
            let version = reader.next().read_der()?;
            // this is the encoding of 2 with context 0
            if version != [160, 3, 2, 1, 2] {
                warn!("got invalid version {:?}", version);
                return Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))?;
            }
            // Skip the serial number
            reader.next().read_der()?;
            if read_algid(&mut reader)? != raw_certificate.algorithm {
                debug!(
                    "expected algid to be {:?}, but it is not",
                    raw_certificate.algorithm
                );
                Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))?
            }
            // Skip the issuer
            reader.next().read_der()?;
            // Skip validity
            reader.next().read_der()?;
            // Skip subject
            reader.next().read_der()?;
            trace!("reading subjectPublicKeyInfo");
            let key = reader.next().read_sequence(|mut reader| {
                trace!("reading subject key algorithm");
                reader.next().read_der()?;
                trace!("reading subject key");
                read_bitvec(&mut reader)
            })?;
            // skip issuerUniqueId
            reader.read_optional(|reader| reader.read_bitvec_bytes())?;
            // skip subjectUniqueId
            reader.read_optional(|reader| reader.read_bitvec_bytes())?;
            trace!("reading extensions");
            parse_x509_extensions(reader, &key)
        })
    })
}

/// The name is a misnomer. We don’t bother checking if the certificate is actually well-formed.
/// We just check that its self-signature is valid, and that its public key is suitably signed.
pub fn verify_libp2p_certificate(
    certificate: &[u8],
    side: Side,
) -> Result<libp2p_core::PeerId, webpki::Error> {
    let trust_anchor = webpki::trust_anchor_util::cert_der_as_trust_anchor(certificate)?;
    let cert = webpki::EndEntityCert::from(certificate)?;
    let time = webpki::Time::try_from(std::time::SystemTime::now()).expect(
        "we assume the system clock is not set to before the UNIX epoch; \
         if it is not, then the system is hopelessly misconfigured, and many \
         other things will break; if this is not set to before the UNIX epoch, \
         this will succeed; qed",
    );
    match side {
        Side::Server => cert.verify_is_valid_tls_server_cert(
            ALL_SUPPORTED_SIGNATURE_ALGORITHMS,
            &webpki::TLSServerTrustAnchors(&[trust_anchor]),
            &[],
            time,
        ),
        Side::Client => cert.verify_is_valid_tls_client_cert(
            ALL_SUPPORTED_SIGNATURE_ALGORITHMS,
            &webpki::TLSClientTrustAnchors(&[trust_anchor]),
            &[],
            time,
        ),
    }?;
    parse_certificate(certificate)
        .map_err(|e| {
            log::debug!("error in parsing: {:?}", e);
            webpki::Error::InvalidSignatureForPublicKey
        })
        .map(From::from)
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn can_make_a_certificate() {
        use Side::{Client, Server};
        drop(env_logger::try_init());
        let keypair = identity::Keypair::generate_ed25519();
        for side in &[Client, Server] {
            assert_eq!(
                verify_libp2p_certificate(&make_cert(&keypair).serialize_der().unwrap(), *side)
                    .unwrap(),
                libp2p_core::PeerId::from_public_key(keypair.public())
            );
        }
        log::trace!("trying secp256k1!");
        let keypair = identity::Keypair::generate_secp256k1();
        log::trace!("have a key!");
        let public = keypair.public();
        log::trace!("have a public key!");
        assert_eq!(public, public, "key is not equal to itself?");
        log::debug!("have a valid key!");
        for side in &[Client, Server] {
            assert_eq!(
                verify_libp2p_certificate(&make_cert(&keypair).serialize_der().unwrap(), *side)
                    .unwrap(),
                libp2p_core::PeerId::from_public_key(keypair.public())
            );
        }
    }
}
