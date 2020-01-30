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
use log::{trace, warn};
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
        reader.read_optional(|reader| reader.read_bool().map(drop))?;
        let contents = reader.next().read_bytes()?;
        match &**oid.components() {
            LIBP2P_OID => verify_libp2p_extension(&contents, certificate_key).map(Some),
            _ => Ok(None),
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
    yasna::parse_der(certificate, |reader| {
        reader.read_sequence(|reader| {
            let key = parse_tbscertificate(reader.next())?;
            // skip algorithm
            reader.next().read_der()?;
            // skip signature
            reader.next().read_der()?;
            Ok(key)
        })
    })
}

fn parse_tbscertificate(reader: yasna::BERReader) -> yasna::ASN1Result<identity::PublicKey> {
    trace!("parsing TBScertificate");
    reader.read_sequence(|reader| {
        // Skip the X.509 version
        reader.next().read_der()?;
        // Skip the serial number
        reader.next().read_der()?;
        // Skip the signature algorithm
        reader.next().read_der()?;
        // Skip the issuer
        reader.next().read_der()?;
        // Skip validity
        reader.next().read_der()?;
        // Skip subject
        reader.next().read_der()?;
        trace!("reading subjectPublicKeyInfo");
        let key = reader.next().read_sequence(|mut reader| {
            // Skip the subject key algorithm
            reader.next().read_der()?;
            trace!("reading subject key");
            read_bitvec(&mut reader)
        })?;
        trace!("reading extensions");
        parse_x509_extensions(reader, &key)
    })
}

/// The name is a misnomer. We don’t bother checking if the certificate is actually well-formed.
/// We just check that its self-signature is valid, and that its public key is suitably signed.
pub fn verify_libp2p_certificate(certificate: &[u8]) -> Result<libp2p_core::PeerId, webpki::Error> {
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
        drop(env_logger::try_init());
        let keypair = identity::Keypair::generate_ed25519();
        assert_eq!(
            verify_libp2p_certificate(&make_cert(&keypair).serialize_der().unwrap()).unwrap(),
            libp2p_core::PeerId::from_public_key(keypair.public())
        );
        log::trace!("trying secp256k1!");
        let keypair = identity::Keypair::generate_secp256k1();
        log::trace!("have a key!");
        let public = keypair.public();
        log::trace!("have a public key!");
        assert_eq!(public, public, "key is not equal to itself?");
        log::debug!("have a valid key!");
        assert_eq!(
            verify_libp2p_certificate(&make_cert(&keypair).serialize_der().unwrap()).unwrap(),
            libp2p_core::PeerId::from_public_key(keypair.public())
        );
    }
}
