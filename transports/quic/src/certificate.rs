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
use log::{debug, trace};
pub const LIBP2P_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 53594, 1, 1];
pub const LIBP2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";
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
    let cert = rcgen::Certificate::from_params(params)
        .expect("certificate generation with valid params will succeed; qed");
    webpki::EndEntityCert::from(&cert.serialize_der().expect("serializing will work; qed"))
        .expect("we just made the certificate, so it is valid; qed");
    cert
}

/// Read a bitvec into a vector of bytes.  Requires the bitvec to be a whole number of bytes.
fn read_bitvec(reader: &mut yasna::BERReaderSeq) -> Result<Vec<u8>, yasna::ASN1Error> {
    let (value, bits) = reader.next().read_bitvec_bytes()?;
    // be extra careful regarding overflow
    if (bits & 7) == 0 && value.len() == (bits >> 3) {
        Ok(value)
    } else {
        debug!("value was of wrong length, sorry!");
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
    #[cfg_attr(test, allow(dead_code))]
    algorithm: yasna::models::ObjectIdentifier,
    #[cfg_attr(test, allow(dead_code))]
    parameters: Option<Vec<u8>>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
pub struct X509Certificate {
    #[cfg_attr(test, allow(dead_code))]
    tbs_certificate: Vec<u8>,
    #[cfg_attr(test, allow(dead_code))]
    algorithm: AlgorithmIdentifier,
    #[cfg_attr(test, allow(dead_code))]
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
            debug!(
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
        if *oid.components() != LIBP2P_OID {
            if is_critical {
                // unknown critical extension
                Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))
            } else {
                Ok(None)
            }
        } else {
            Ok(Some(verify_libp2p_extension(&contents, certificate_key)?))
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

fn compute_signature_algorithm(
    key: &[u8],
    alg: &AlgorithmIdentifier,
) -> yasna::ASN1Result<&'static webpki::SignatureAlgorithm> {
    #![cfg_attr(not(test), allow(dead_code))]
    use webpki::*;
    Ok(match (key.len(), &**alg.algorithm.components()) {
        (32, &[1, 3, 101, 111]) => &ED25519,
        (33, &[1, 2, 840, 10045, 4, 3, 2]) => &ECDSA_P256_SHA256,
        (65, &[1, 2, 840, 10045, 4, 3, 2]) => &ECDSA_P256_SHA256,
        (33, &[1, 2, 840, 10045, 4, 3, 3]) => &ECDSA_P256_SHA384,
        (65, &[1, 2, 840, 10045, 4, 3, 3]) => &ECDSA_P256_SHA384,
        (49, &[1, 2, 840, 10045, 4, 3, 2]) => &ECDSA_P384_SHA256,
        (97, &[1, 2, 840, 10045, 4, 3, 2]) => &ECDSA_P384_SHA256,
        (49, &[1, 2, 840, 10045, 4, 3, 3]) => &ECDSA_P384_SHA384,
        (97, &[1, 2, 840, 10045, 4, 3, 3]) => &ECDSA_P384_SHA384,
        (_, &[1, 2, 840, 113549, 1, 1, 11]) => &RSA_PKCS1_2048_8192_SHA256,
        (_, &[1, 2, 840, 113549, 1, 1, 12]) => &RSA_PKCS1_2048_8192_SHA384,
        (_, &[1, 2, 840, 113549, 1, 1, 13]) => &RSA_PKCS1_2048_8192_SHA512,
        (_, &[1, 2, 840, 113549, 1, 1, 10]) => match alg.parameters {
            None => return Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid)),
            Some(ref e) => yasna::parse_der(e, |reader| {
                reader.read_sequence(|reader| {
                    let keytype = reader.next().read_sequence(|reader| {
                        let keytype = match &**reader.next().read_oid()?.components() {
                            &[2, 16, 840, 1, 101, 3, 4, 2, 1] => {
                                &RSA_PSS_2048_8192_SHA256_LEGACY_KEY
                            }
                            &[2, 16, 840, 1, 101, 3, 4, 2, 2] => {
                                &RSA_PSS_2048_8192_SHA384_LEGACY_KEY
                            }
                            &[2, 16, 840, 1, 101, 3, 4, 2, 3] => {
                                &RSA_PSS_2048_8192_SHA512_LEGACY_KEY
                            }
                            _ => return Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid)),
                        };
                        reader.read_optional(|reader| reader.read_null())?;
                        Ok(keytype)
                    })?;
                    reader.next().read_der()?; // ignore maskGenAlgorithm
                    reader.next().read_der()?; // ignore saltLength
                    Ok(keytype)
                })
            })?,
        },
        _ => return Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid)),
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn parse_certificate(
    certificate: &[u8],
) -> yasna::ASN1Result<(X509Certificate, Vec<u8>, identity::PublicKey)> {
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
    let (certificate_key, identity_key) =
        yasna::parse_der(&raw_certificate.tbs_certificate, |reader| {
            trace!("parsing TBScertificate");
            reader.read_sequence(|mut reader| {
                trace!("getting X509 version");
                let version = reader.next().read_der()?;
                // this is the encoding of 2 with context 0
                if version != [160, 3, 2, 1, 2] {
                    debug!("got invalid version {:?}", version);
                    return Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))?;
                }
                let serial_number = reader.next().read_biguint()?;
                trace!("got serial number {:?}", serial_number);
                drop(serial_number);
                if read_algid(&mut reader)? != raw_certificate.algorithm {
                    debug!(
                        "expected algid to be {:?}, but it is not",
                        raw_certificate.algorithm
                    );
                    Err(yasna::ASN1Error::new(yasna::ASN1ErrorKind::Invalid))?
                }
                reader.next().read_der()?; // we don’t care about the issuer
                trace!("reading validity");
                reader.next().read_der()?; // we don’t care about the validity
                trace!("reading subject");
                reader.next().read_der()?; // we don’t care about the subject
                trace!("reading subjectPublicKeyInfo");
                let key = reader.next().read_sequence(|mut reader| {
                    trace!("reading subject key algorithm");
                    reader.next().read_der()?;
                    trace!("reading subject key");
                    read_bitvec(&mut reader)
                })?;
                trace!("reading issuerUniqueId");
                reader.read_optional(|reader| reader.read_bitvec_bytes())?; // we don’t care about the issuerUniqueId
                trace!("reading subjectUniqueId");
                reader.read_optional(|reader| reader.read_bitvec_bytes())?; // we don’t care about the subjectUniqueId
                trace!("reading extensions");
                let identity_key = parse_x509_extensions(reader, &key)?;
                Ok((key, identity_key))
            })
        })?;
    Ok((raw_certificate, certificate_key, identity_key))
}

pub fn verify_libp2p_certificate(certificate: &[u8]) -> Result<identity::PublicKey, ()> {
    #![cfg_attr(not(test), allow(dead_code))]
    let end_entity_cert = webpki::EndEntityCert::from(certificate)
        .map_err(|e| log::debug!("webpki found invalid certificate: {:?}", e))?;
    let (raw_certificate, certificate_key, identity_key): (_, Vec<u8>, _) =
        parse_certificate(certificate).map_err(|e| log::debug!("error in parsing: {:?}", e))?;
    let algorithm = compute_signature_algorithm(&certificate_key, &raw_certificate.algorithm)
        .map_err(|e| log::debug!("error getting signature algorithm: {:?}", e))?;
    end_entity_cert
        .verify_signature(
            &algorithm,
            &raw_certificate.tbs_certificate,
            &raw_certificate.signature_value,
        )
        .map_err(|e| log::debug!("error from webpki: {:?}", e))?;
    Ok(identity_key)
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn can_make_a_certificate() {
        drop(env_logger::try_init());
        let keypair = identity::Keypair::generate_ed25519();
        assert_eq!(
            verify_libp2p_certificate(&make_cert(&keypair).serialize_der().unwrap()).unwrap(),
            keypair.public()
        );
        log::trace!("trying secp256k1!");
        let keypair = identity::Keypair::generate_secp256k1();
        log::trace!("have a key!");
        let public = keypair.public();
        log::trace!("have a public key!");
        assert_eq!(public, public, "key is not equal to itself?");
        // assert_eq!(
        //     identity::PublicKey::from_protobuf_encoding(&public.clone().into_protobuf_encoding())
        //         .unwrap(),
        //     public,
        //     "encoding and decoding corrupted key!"
        // );
        log::error!("have a valid key!");
        assert_eq!(
            verify_libp2p_certificate(&make_cert(&keypair).serialize_der().unwrap(),)
                .unwrap()
                .into_protobuf_encoding(),
            keypair.public().into_protobuf_encoding()
        );
    }
}
