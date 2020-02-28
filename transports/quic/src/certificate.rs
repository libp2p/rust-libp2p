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

use libp2p_core::identity;
const LIBP2P_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 53594, 1, 1];
const LIBP2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";
const LIBP2P_SIGNING_PREFIX_LENGTH: usize = LIBP2P_SIGNING_PREFIX.len();
const LIBP2P_SIGNATURE_ALGORITHM_PUBLIC_KEY_LENGTH: usize = 65;
static LIBP2P_SIGNATURE_ALGORITHM: &rcgen::SignatureAlgorithm = &rcgen::PKCS_ECDSA_P256_SHA256;

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
    let mut ext = rcgen::CustomExtension::from_oid_content(LIBP2P_OID, contents);
    ext.set_criticality(true);
    ext
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

/// Generates a self-signed TLS certificate that includes a libp2p-specific certificate extension
/// containing the public key of the given keypair.
pub(crate) fn make_cert(keypair: &identity::Keypair) -> rcgen::Certificate {
    let mut params = rcgen::CertificateParams::new(vec![]);
    let (cert_keypair, libp2p_extension) = gen_signed_keypair(keypair);
    params.custom_extensions.push(libp2p_extension);
    params.alg = &LIBP2P_SIGNATURE_ALGORITHM;
    params.key_pair = Some(cert_keypair);
    rcgen::Certificate::from_params(params)
        .expect("certificate generation with valid params will succeed; qed")
}

/// Extract the peer’s public key from a libp2p certificate. It is erroneous to
/// call this unless the certificate is known to be a valid libp2p certificate.
/// The certificate verifiers in this crate validate that a certificate is in
/// fact a valid libp2p certificate.
///
/// # Panics
///
/// Panics if the key could not be extracted.
pub(crate) fn extract_libp2p_peerid(certificate: &[u8]) -> libp2p_core::PeerId {
    let mut id = None;
    webpki::EndEntityCert::from_with_extension_cb(
        certificate,
        &mut |oid, value, _critical, _spki| match oid.as_slice_less_safe() {
            [43, 6, 1, 4, 1, 131, 162, 90, 1, 1] => {
                use ring::io::der;
                value
                    .read_all(ring::error::Unspecified, |mut reader| {
                        der::expect_tag_and_get_value(&mut reader, der::Tag::Sequence)?.read_all(
                            ring::error::Unspecified,
                            |mut reader| {
                                let public_key = der::bit_string_with_no_unused_bits(&mut reader)?
                                    .as_slice_less_safe();
                                let _signature = der::bit_string_with_no_unused_bits(&mut reader)?;
                                id = Some(
                                    identity::PublicKey::from_protobuf_encoding(public_key)
                                        .map_err(|_| ring::error::Unspecified)?,
                                );
                                Ok(())
                            },
                        )
                    })
                    .expect("we already validated the certificate");
                webpki::Understood::Yes
            }
            _ => webpki::Understood::No,
        },
    )
    .expect("we already validated the certificate");
    id.expect("we already checked that a PeerId exists").into()
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn can_make_a_certificate() {
        drop(env_logger::try_init());
        let keypair = identity::Keypair::generate_ed25519();
        assert_eq!(
            extract_libp2p_peerid(&make_cert(&keypair).serialize_der().unwrap()),
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
            extract_libp2p_peerid(&make_cert(&keypair).serialize_der().unwrap()),
            libp2p_core::PeerId::from_public_key(keypair.public())
        );
    }
}
