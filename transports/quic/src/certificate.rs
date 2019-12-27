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

//! The libp2p extension OID
pub const LIBP2P_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 53594, 1, 1];
pub const LIBP2P_SIGNING_PREFIX: [u8; 21] = *b"libp2p-tls-handshake:";
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
    let mut libp2p_extension = rcgen::CustomExtension::from_oid_content(LIBP2P_OID, contents);
    libp2p_extension.set_criticality(true);
    libp2p_extension
}

fn gen_signed_keypair(keypair: &identity::Keypair) -> (rcgen::KeyPair, rcgen::CustomExtension) {
    let temp_keypair = rcgen::KeyPair::generate(&LIBP2P_SIGNATURE_ALGORITHM)
        .expect("we pass valid parameters, and assume we have enough memory and randomness; qed");
    let mut signing_buf = [0u8; 21 + LIBP2P_SIGNATURE_ALGORITHM_PUBLIC_KEY_LENGTH];
    let public = temp_keypair.public_key_raw();
    assert_eq!(public.len(), LIBP2P_SIGNATURE_ALGORITHM_PUBLIC_KEY_LENGTH, "ECDSA public keys are 65 bytes");
    signing_buf[..21].copy_from_slice(&LIBP2P_SIGNING_PREFIX[..]);
    signing_buf[21..].copy_from_slice(public);
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
        .expect("certificate generation with valid params will succeed")
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn can_make_a_certificate() {
        make_cert(&identity::Keypair::generate_ed25519());
        make_cert(&identity::Keypair::generate_secp256k1());
    }
}
