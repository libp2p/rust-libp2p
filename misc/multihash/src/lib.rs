//! # Multihash
//!
//! Implementation of [multihash](https://github.com/multiformats/multihash) in Rust.
//!
//! A `Multihash` is a structure that contains a hashing algorithm, plus some hashed data.
//! A `MultihashRef` is the same as a `Multihash`, except that it doesn't own its data.

mod errors;
mod hashes;

use bytes::{BufMut, Bytes, BytesMut};
use rand::RngCore;
use sha2::Digest;
use std::{convert::TryFrom, fmt::Write};
use unsigned_varint::{decode, encode};

pub use self::errors::{DecodeError, DecodeOwnedError, EncodeError};
pub use self::hashes::Hash;

/// Helper function for encoding input into output using given `Digest`
fn digest_encode<D: Digest>(input: &[u8], output: &mut [u8]) {
    output.copy_from_slice(&D::digest(input))
}

// And another one to keep the matching DRY
macro_rules! match_encoder {
    ($hash_id:ident for ($input:expr, $output:expr) {
        $( $hashtype:ident => $hash_ty:path, )*
    }) => ({
        match $hash_id {
            $(
                Hash::$hashtype => digest_encode::<$hash_ty>($input, $output),
            )*

            _ => return Err(EncodeError::UnsupportedType)
        }
    })
}

/// Encodes data into a multihash.
///
/// # Errors
///
/// Will return an error if the specified hash type is not supported. See the docs for `Hash`
/// to see what is supported.
///
/// # Examples
///
/// ```
/// use parity_multihash::{encode, Hash};
///
/// assert_eq!(
///     encode(Hash::SHA2256, b"hello world").unwrap().to_vec(),
///     vec![18, 32, 185, 77, 39, 185, 147, 77, 62, 8, 165, 46, 82, 215, 218, 125, 171, 250, 196,
///     132, 239, 227, 122, 83, 128, 238, 144, 136, 247, 172, 226, 239, 205, 233]
/// );
/// ```
///
pub fn encode(hash: Hash, input: &[u8]) -> Result<Multihash, EncodeError> {
    // Custom length encoding for the identity multihash
    if let Hash::Identity = hash {
        if u64::from(std::u32::MAX) < as_u64(input.len()) {
            return Err(EncodeError::UnsupportedInputLength);
        }
        let mut buf = encode::u16_buffer();
        let code = encode::u16(hash.code(), &mut buf);
        let mut len_buf = encode::u32_buffer();
        let size = encode::u32(input.len() as u32, &mut len_buf);

        let total_len = code.len() + size.len() + input.len();

        let mut output = BytesMut::with_capacity(total_len);
        output.put_slice(code);
        output.put_slice(size);
        output.put_slice(input);
        Ok(Multihash {
            bytes: output.freeze(),
        })
    } else {
        let (offset, mut output) = encode_hash(hash);
        match_encoder!(hash for (input, &mut output[offset ..]) {
            SHA1 => sha1::Sha1,
            SHA2256 => sha2::Sha256,
            SHA2512 => sha2::Sha512,
            SHA3224 => sha3::Sha3_224,
            SHA3256 => sha3::Sha3_256,
            SHA3384 => sha3::Sha3_384,
            SHA3512 => sha3::Sha3_512,
            Keccak224 => sha3::Keccak224,
            Keccak256 => sha3::Keccak256,
            Keccak384 => sha3::Keccak384,
            Keccak512 => sha3::Keccak512,
            Blake2b512 => blake2::Blake2b,
            Blake2s256 => blake2::Blake2s,
        });
        Ok(Multihash {
            bytes: output.freeze(),
        })
    }
}

// Encode the given [`Hash`] value and ensure the returned [`BytesMut`]
// has enough capacity to hold the actual digest.
fn encode_hash(hash: Hash) -> (usize, BytesMut) {
    let mut buf = encode::u16_buffer();
    let code = encode::u16(hash.code(), &mut buf);

    let len = code.len() + 1 + usize::from(hash.size());

    let mut output = BytesMut::with_capacity(len);
    output.put_slice(code);
    output.put_u8(hash.size());
    output.resize(len, 0);

    (code.len() + 1, output)
}

/// Represents a valid multihash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Multihash { bytes: Bytes }

impl Multihash {
    /// Verifies whether `bytes` contains a valid multihash, and if so returns a `Multihash`.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Multihash, DecodeOwnedError> {
        if let Err(err) = MultihashRef::from_slice(&bytes) {
            return Err(DecodeOwnedError { error: err, data: bytes });
        }
        Ok(Multihash { bytes: Bytes::from(bytes) })
    }

    /// Generates a random `Multihash` from a cryptographically secure PRNG.
    pub fn random(hash: Hash) -> Multihash {
        let (offset, mut bytes) = encode_hash(hash);
        rand::thread_rng().fill_bytes(&mut bytes[offset ..]);
        Multihash { bytes: bytes.freeze() }
    }

    /// Returns the bytes representation of the multihash.
    pub fn into_bytes(self) -> Vec<u8> {
        self.to_vec()
    }

    /// Returns the bytes representation of the multihash.
    pub fn to_vec(&self) -> Vec<u8> {
        Vec::from(&self.bytes[..])
    }

    /// Returns the bytes representation of this multihash.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Builds a `MultihashRef` corresponding to this `Multihash`.
    pub fn as_ref(&self) -> MultihashRef<'_> {
        MultihashRef { bytes: &self.bytes }
    }

    /// Returns which hashing algorithm is used in this multihash.
    pub fn algorithm(&self) -> Hash {
        self.as_ref().algorithm()
    }

    /// Returns the hashed data.
    pub fn digest(&self) -> &[u8] {
        self.as_ref().digest()
    }
}

impl AsRef<[u8]> for Multihash {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<'a> PartialEq<MultihashRef<'a>> for Multihash {
    fn eq(&self, other: &MultihashRef<'a>) -> bool {
        &*self.bytes == other.bytes
    }
}

impl TryFrom<Vec<u8>> for Multihash {
    type Error = DecodeOwnedError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Multihash::from_bytes(value)
    }
}

/// Represents a valid multihash.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct MultihashRef<'a> { bytes: &'a [u8] }

impl<'a> MultihashRef<'a> {
    /// Creates a `MultihashRef` from the given `input`.
    pub fn from_slice(input: &'a [u8]) -> Result<Self, DecodeError> {
        if input.is_empty() {
            return Err(DecodeError::BadInputLength)
        }

        // Ensure `Hash::code` returns a `u16` so that our `decode::u16` here is correct.
        std::convert::identity::<fn(&'_ Hash) -> u16>(Hash::code);
        let (code, bytes) = decode::u16(&input).map_err(|_| DecodeError::BadInputLength)?;

        let alg = Hash::from_code(code).ok_or(DecodeError::UnknownCode)?;

        // handle the identity case
        if alg == Hash::Identity {
            let (hash_len, bytes) = decode::u32(&bytes).map_err(|_| DecodeError::BadInputLength)?;
            if as_u64(bytes.len()) != u64::from(hash_len) {
                return Err(DecodeError::BadInputLength);
            }
            return Ok(MultihashRef { bytes: input });
        }

        let hash_len = usize::from(alg.size());

        // Length of input after hash code should be exactly hash_len + 1
        if bytes.len() != hash_len + 1 {
            return Err(DecodeError::BadInputLength);
        }

        if usize::from(bytes[0]) != hash_len {
            return Err(DecodeError::BadInputLength);
        }

        Ok(MultihashRef { bytes: input })
    }

    /// Returns which hashing algorithm is used in this multihash.
    pub fn algorithm(&self) -> Hash {
        let code = decode::u16(&self.bytes)
            .expect("multihash is known to be valid algorithm")
            .0;
        Hash::from_code(code).expect("multihash is known to be valid")
    }

    /// Returns the hashed data.
    pub fn digest(&self) -> &'a [u8] {
        let bytes = decode::u16(&self.bytes)
            .expect("multihash is known to be valid digest")
            .1;
        &bytes[1 ..]
    }

    /// Builds a `Multihash` that owns the data.
    ///
    /// This operation allocates.
    pub fn into_owned(self) -> Multihash {
        Multihash {
            bytes: Bytes::from(self.bytes)
        }
    }

    /// Returns the bytes representation of this multihash.
    pub fn as_bytes(&self) -> &'a [u8] {
        &self.bytes
    }
}

impl<'a> PartialEq<Multihash> for MultihashRef<'a> {
    fn eq(&self, other: &Multihash) -> bool {
        self.bytes == &*other.bytes
    }
}

#[cfg(any(target_pointer_width = "32", target_pointer_width = "64"))]
fn as_u64(a: usize) -> u64 {
    a as u64
}

/// Convert bytes to a hex representation
pub fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        write!(hex, "{:02x}", byte).expect("Can't fail on writing to string");
    }

    hex
}

#[cfg(test)]
mod tests {
    use crate::{Hash, Multihash};
    use std::convert::TryFrom;

    #[test]
    fn rand_generates_valid_multihash() {
        // Iterate over every possible hash function.
        for code in 0 .. u16::max_value() {
            let hash_fn = match Hash::from_code(code) {
                Some(c) => c,
                None => continue,
            };

            for _ in 0 .. 2000 {
                let hash = Multihash::random(hash_fn);
                assert_eq!(hash, Multihash::try_from(hash.to_vec()).unwrap());
            }
        }
    }
}
