// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! Ecdsa keys.

use super::error::{DecodingError, SigningError};
use core::fmt;
use ring::{
    rand::SystemRandom,
    signature::{
        self, EcdsaKeyPair as EcdsaKeyPairImpl, EcdsaSigningAlgorithm, EcdsaVerificationAlgorithm,
        KeyPair as RingKeyPair, UnparsedPublicKey as PublicKeyImpl,
    },
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum CurveId {
    Secp256r1,
}

/// An ECDSA keypair.
#[derive(Clone)]
pub struct Keypair {
    curve_id: CurveId,
    pair: Arc<EcdsaKeyPairImpl>,
}

impl Keypair {
    /// Generate a new random ECDSA p256 keypair.
    pub fn generate(curve_id: CurveId) -> Keypair {
        let rng = SystemRandom::new();
        let alg = get_sign_alg(curve_id);
        let pkcs8 = EcdsaKeyPairImpl::generate_pkcs8(alg, &rng).unwrap();
        let imp = EcdsaKeyPairImpl::from_pkcs8(alg, pkcs8.as_ref()).unwrap();
        Keypair {
            curve_id,
            pair: Arc::new(imp),
        }
    }

    /// Sign a message using the private key of this keypair.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let rng = SystemRandom::new();
        self.pair.sign(&rng, msg).unwrap().as_ref().to_vec()
    }

    // Get the public key of this keypair.
    pub fn public(&self) -> PublicKey {
        PublicKey {
            curve_id: self.curve_id,
            key: self.pair.public_key().as_ref().to_owned(),
        }
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair")
            .field("public", &self.public())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    curve_id: CurveId,
    key: Vec<u8>,
}

impl PublicKey {
    /// Verify the ecdsa signature on a message using the public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        let alg = get_verify_alg(self.curve_id);
        let pk = PublicKeyImpl::new(alg, &self.key);
        pk.verify(msg, sig).is_ok()
    }

    // Encode a public key in ASN.1 format byte buffer.
    pub fn encode_asn1(&self) -> Vec<u8> {
        let bytes = self.key.clone();
        add_asn1_header(self.curve_id, bytes)
    }

    // Decode a public key from an ASN.1 byte buffer.
    pub fn decode(k: &[u8]) -> Result<PublicKey, DecodingError> {
        let curve_id = CurveId::Secp256r1;
        let key = del_asn1_header(k.to_owned())?;
        Ok(PublicKey { curve_id, key })
        //     .map_err(|_| DecodingError::new("failed to parse secp256k1 public key"))
        //     .map(PublicKey)
    }
}

fn get_sign_alg(curve_id: CurveId) -> &'static EcdsaSigningAlgorithm {
    match curve_id {
        CurveId::Secp256r1 => &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
    }
}

fn get_verify_alg(curve_id: CurveId) -> &'static EcdsaVerificationAlgorithm {
    match curve_id {
        CurveId::Secp256r1 => &signature::ECDSA_P256_SHA256_FIXED,
    }
}

fn del_asn1_header(asn1_bytes: Vec<u8>) -> Result<Vec<u8>, DecodingError> {
    let key_len = 65;
    let key = &asn1_bytes[asn1_bytes.len() - key_len..];
    Ok(key.to_owned())
}

// SECG named elliptic curve)
fn get_curve_oid(curve_id: CurveId) -> Vec<u8> {
    match curve_id {
        // secp256r1 OID: 1.2.840.10045.3.1.7
        CurveId::Secp256r1 => vec![0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07],
        // secp384r1 OID: 1.3.132.0.34
        //CurveId::Secp384r1 => vec![0x06, 0x05, 0x2b, 0x81, 0x04, 0x00, 0x22],
    }
}

// ASN1 header.
#[rustfmt::skip]
fn add_asn1_header(curve_id: CurveId, mut key_bytes: Vec<u8>) -> Vec<u8> {
    let mut res = vec![
        // ASN.1 struct type and length.
        0x30, 0x00,
        // ASN.1 struct type and length.
        0x30, 0x00,
    ];

    // ecPublicKey (ANSI X9.62 public key type) OID: 1.2.840.10045.2.1 
    let mut ec_oid = vec![ 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01 ];
    // curve OID
    let mut curve_oid = get_curve_oid(curve_id);

    let oids_len = ec_oid.len() + curve_oid.len();
    res.append(&mut ec_oid);
    res.append(&mut curve_oid);

    // Update oids length field
    res[3] = oids_len as u8;

    // Append key bitstring type and length.
    let mut bitstring_type_len = vec![
        0x03, (key_bytes.len() + 1) as u8, 0x00,
    ];
    res.append(&mut bitstring_type_len);
    // Append key bitstring.
    res.append(&mut key_bytes);
    // Update overall length field.
    res[1] = (res.len() - 2) as u8;

    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecdsa_generate() {
        let pair = Keypair::generate(CurveId::Secp256r1);

        let public = pair.public();
        println!("{:?}", public);
    }
}
