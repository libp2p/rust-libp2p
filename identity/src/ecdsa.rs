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

//! ECDSA keys with secp256r1 curve support.

use super::error::DecodingError;
use core::cmp;
use core::fmt;
use core::hash;
use p256::{
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    EncodedPoint,
};
use sec1::{DecodeEcPrivateKey, EncodeEcPrivateKey};
use void::Void;
use zeroize::Zeroize;

/// An ECDSA keypair generated using `secp256r1` curve.
#[derive(Clone)]
pub struct Keypair {
    secret: SecretKey,
    public: PublicKey,
}

impl Keypair {
    /// Generate a new random ECDSA keypair.
    pub fn generate() -> Keypair {
        Keypair::from(SecretKey::generate())
    }

    /// Sign a message using the private key of this keypair.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        self.secret.sign(msg)
    }

    /// Get the public key of this keypair.
    pub fn public(&self) -> &PublicKey {
        &self.public
    }

    /// Get the secret key of this keypair.
    pub fn secret(&self) -> &SecretKey {
        &self.secret
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair")
            .field("public", &self.public())
            .finish()
    }
}

/// Promote an ECDSA secret key into a keypair.
impl From<SecretKey> for Keypair {
    fn from(secret: SecretKey) -> Keypair {
        let public = PublicKey(VerifyingKey::from(&secret.0));
        Keypair { secret, public }
    }
}

/// Demote an ECDSA keypair to a secret key.
impl From<Keypair> for SecretKey {
    fn from(kp: Keypair) -> SecretKey {
        kp.secret
    }
}

/// An ECDSA secret key generated using `secp256r1` curve.
#[derive(Clone)]
pub struct SecretKey(SigningKey);

impl SecretKey {
    /// Generate a new random ECDSA secret key.
    pub fn generate() -> SecretKey {
        SecretKey(SigningKey::random(&mut rand::thread_rng()))
    }

    /// Sign a message with this secret key, producing a DER-encoded ECDSA signature.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let signature: p256::ecdsa::DerSignature = self.0.sign(msg);

        signature.as_bytes().to_owned()
    }

    /// Convert a secret key into a byte buffer containing raw scalar of the key.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }

    /// Try to parse a secret key from a byte buffer containing raw scalar of the key.
    pub fn try_from_bytes(buf: impl AsRef<[u8]>) -> Result<SecretKey, DecodingError> {
        SigningKey::from_bytes(buf.as_ref().into())
            .map_err(|err| DecodingError::failed_to_parse("ecdsa p256 secret key", err))
            .map(SecretKey)
    }

    /// Encode the secret key into DER-encoded byte buffer.
    pub(crate) fn encode_der(&self) -> Vec<u8> {
        self.0
            .to_sec1_der()
            .expect("Encoding to pkcs#8 format to succeed")
            .to_bytes()
            .to_vec()
    }

    /// Try to decode a secret key from a DER-encoded byte buffer, zeroize the buffer on success.
    pub(crate) fn try_decode_der(buf: &mut [u8]) -> Result<Self, DecodingError> {
        match SigningKey::from_sec1_der(buf) {
            Ok(key) => {
                buf.zeroize();
                Ok(SecretKey(key))
            }
            Err(e) => Err(DecodingError::failed_to_parse("ECDSA", e)),
        }
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey")
    }
}

/// An ECDSA public key.
#[derive(Clone, Eq, PartialOrd, Ord)]
pub struct PublicKey(VerifyingKey);

impl PublicKey {
    /// Verify an ECDSA signature on a message using the public key.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        let sig = match Signature::from_der(sig) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        self.0.verify(msg, &sig).is_ok()
    }

    /// Try to parse a public key from a byte buffer containing raw components of a key with or without compression.
    pub fn try_from_bytes(k: &[u8]) -> Result<PublicKey, DecodingError> {
        let enc_pt = EncodedPoint::from_bytes(k)
            .map_err(|e| DecodingError::failed_to_parse("ecdsa p256 encoded point", e))?;

        VerifyingKey::from_encoded_point(&enc_pt)
            .map_err(|err| DecodingError::failed_to_parse("ecdsa p256 public key", err))
            .map(PublicKey)
    }

    /// Convert a public key into a byte buffer containing raw components of the key without compression.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_encoded_point(false).as_bytes().to_owned()
    }

    /// Encode a public key into a DER encoded byte buffer as defined by SEC1 standard.
    pub fn encode_der(&self) -> Vec<u8> {
        let buf = self.to_bytes();
        Self::add_asn1_header(&buf)
    }

    /// Try to decode a public key from a DER encoded byte buffer as defined by SEC1 standard.
    pub fn try_decode_der(k: &[u8]) -> Result<PublicKey, DecodingError> {
        let buf = Self::del_asn1_header(k).ok_or_else(|| {
            DecodingError::failed_to_parse::<Void, _>("ASN.1-encoded ecdsa p256 public key", None)
        })?;
        Self::try_from_bytes(buf)
    }

    // ecPublicKey (ANSI X9.62 public key type) OID: 1.2.840.10045.2.1
    const EC_PUBLIC_KEY_OID: [u8; 9] = [0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
    // secp256r1 OID: 1.2.840.10045.3.1.7
    const SECP_256_R1_OID: [u8; 10] = [0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

    // Add ASN1 header.
    fn add_asn1_header(key_buf: &[u8]) -> Vec<u8> {
        // ASN.1 struct type and length.
        let mut asn1_buf = vec![
            0x30,
            0x00,
            0x30,
            (Self::EC_PUBLIC_KEY_OID.len() + Self::SECP_256_R1_OID.len()) as u8,
        ];
        // Append OIDs.
        asn1_buf.extend_from_slice(&Self::EC_PUBLIC_KEY_OID);
        asn1_buf.extend_from_slice(&Self::SECP_256_R1_OID);
        // Append key bitstring type and length.
        asn1_buf.extend_from_slice(&[0x03, (key_buf.len() + 1) as u8, 0x00]);
        // Append key bitstring value.
        asn1_buf.extend_from_slice(key_buf);
        // Update overall length field.
        asn1_buf[1] = (asn1_buf.len() - 2) as u8;

        asn1_buf
    }

    // Check and remove ASN.1 header.
    fn del_asn1_header(asn1_buf: &[u8]) -> Option<&[u8]> {
        let oids_len = Self::EC_PUBLIC_KEY_OID.len() + Self::SECP_256_R1_OID.len();
        let asn1_head = asn1_buf.get(..4)?;
        let oids_buf = asn1_buf.get(4..4 + oids_len)?;
        let bitstr_head = asn1_buf.get(4 + oids_len..4 + oids_len + 3)?;

        // Sanity check
        if asn1_head[0] != 0x30
            || asn1_head[2] != 0x30
            || asn1_head[3] as usize != oids_len
            || oids_buf[..Self::EC_PUBLIC_KEY_OID.len()] != Self::EC_PUBLIC_KEY_OID
            || oids_buf[Self::EC_PUBLIC_KEY_OID.len()..] != Self::SECP_256_R1_OID
            || bitstr_head[0] != 0x03
            || bitstr_head[2] != 0x00
        {
            return None;
        }

        let key_len = bitstr_head[1].checked_sub(1)? as usize;
        let key_buf = asn1_buf.get(4 + oids_len + 3..4 + oids_len + 3 + key_len)?;
        Some(key_buf)
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublicKey(asn.1 uncompressed): ")?;
        for byte in &self.encode_der() {
            write!(f, "{byte:x}")?;
        }
        Ok(())
    }
}

impl cmp::PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.to_bytes().eq(&other.to_bytes())
    }
}

impl hash::Hash for PublicKey {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.to_bytes().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify() {
        let pair = Keypair::generate();
        let pk = pair.public();

        let msg = "hello world".as_bytes();
        let sig = pair.sign(msg);
        assert!(pk.verify(msg, &sig));

        let mut invalid_sig = sig.clone();
        invalid_sig[3..6].copy_from_slice(&[10, 23, 42]);
        assert!(!pk.verify(msg, &invalid_sig));

        let invalid_msg = "h3ll0 w0rld".as_bytes();
        assert!(!pk.verify(invalid_msg, &sig));
    }
}
