//! # Multihash
//!
//! Implementation of [multihash](https://github.com/multiformats/multihash) in Rust.
//!
//! A `Multihash` is a structure that contains a hashing algorithm, plus some hashed data.
//! A `MultihashRef` is the same as a `Multihash`, except that it doesn't own its data.

extern crate blake2;
extern crate rand;
extern crate sha1;
extern crate sha2;
extern crate tiny_keccak;
extern crate unsigned_varint;

mod errors;
mod hashes;

use std::fmt::Write;

use sha2::Digest;
use tiny_keccak::Keccak;
use unsigned_varint::{decode, encode};

pub use errors::{DecodeError, DecodeOwnedError, EncodeError};
pub use hashes::Hash;

// Helper macro for encoding input into output using sha1, sha2, tiny_keccak, or blake2
macro_rules! encode {
    (sha1, Sha1, $input:expr, $output:expr) => {{
        let mut hasher = sha1::Sha1::new();
        hasher.update($input);
        $output.copy_from_slice(&hasher.digest().bytes());
    }};
    (sha2, $algorithm:ident, $input:expr, $output:expr) => {{
        let mut hasher = sha2::$algorithm::default();
        hasher.input($input);
        $output.copy_from_slice(hasher.result().as_ref());
    }};
    (tiny, $constructor:ident, $input:expr, $output:expr) => {{
        let mut kec = Keccak::$constructor();
        kec.update($input);
        kec.finalize($output);
    }};
    (blake2, $algorithm:ident, $input:expr, $output:expr) => {{
        let mut hasher = blake2::$algorithm::default();
        hasher.input($input);
        $output.copy_from_slice(hasher.result().as_ref());
    }};
}

// And another one to keep the matching DRY
macro_rules! match_encoder {
    ($hash:ident for ($input:expr, $output:expr) {
        $( $hashtype:ident => $lib:ident :: $method:ident, )*
    }) => ({
        match $hash {
            $(
                Hash::$hashtype => encode!($lib, $method, $input, $output),
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
/// use multihash::{encode, Hash};
///
/// assert_eq!(
///     encode(Hash::SHA2256, b"hello world").unwrap().into_bytes(),
///     vec![18, 32, 185, 77, 39, 185, 147, 77, 62, 8, 165, 46, 82, 215, 218, 125, 171, 250, 196,
///     132, 239, 227, 122, 83, 128, 238, 144, 136, 247, 172, 226, 239, 205, 233]
/// );
/// ```
///
pub fn encode(hash: Hash, input: &[u8]) -> Result<Multihash, EncodeError> {
    let mut buf = encode::u16_buffer();
    let code = encode::u16(hash.code(), &mut buf);

    let header_len = code.len() + 1;
    let size = hash.size();

    let mut output = Vec::new();
    output.resize(header_len + size as usize, 0);
    output[..code.len()].copy_from_slice(code);
    output[code.len()] = size;

    match_encoder!(hash for (input, &mut output[header_len..]) {
        SHA1 => sha1::Sha1,
        SHA2256 => sha2::Sha256,
        SHA2512 => sha2::Sha512,
        SHA3224 => tiny::new_sha3_224,
        SHA3256 => tiny::new_sha3_256,
        SHA3384 => tiny::new_sha3_384,
        SHA3512 => tiny::new_sha3_512,
        Keccak224 => tiny::new_keccak224,
        Keccak256 => tiny::new_keccak256,
        Keccak384 => tiny::new_keccak384,
        Keccak512 => tiny::new_keccak512,
        Blake2b512 => blake2::Blake2b,
        Blake2s256 => blake2::Blake2s,
    });

    Ok(Multihash { bytes: output })
}

/// Represents a valid multihash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Multihash {
    bytes: Vec<u8>,
}

impl Multihash {
    /// Verifies whether `bytes` contains a valid multihash, and if so returns a `Multihash`.
    #[inline]
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Multihash, DecodeOwnedError> {
        if let Err(err) = MultihashRef::from_slice(&bytes) {
            return Err(DecodeOwnedError {
                error: err,
                data: bytes,
            });
        }

        Ok(Multihash { bytes })
    }

    /// Generates a random `Multihash` from a cryptographically secure PRNG.
    pub fn random(hash: Hash) -> Multihash {
        let mut buf = encode::u16_buffer();
        let code = encode::u16(hash.code(), &mut buf);

        let header_len = code.len() + 1;
        let size = hash.size();

        let mut output = Vec::new();
        output.resize(header_len + size as usize, 0);
        output[..code.len()].copy_from_slice(code);
        output[code.len()] = size;

        for b in output[header_len..].iter_mut() {
            *b = rand::random();
        }

        Multihash {
            bytes: output,
        }
    }

    /// Returns the bytes representation of the multihash.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Returns the bytes representation of this multihash.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Builds a `MultihashRef` corresponding to this `Multihash`.
    #[inline]
    pub fn as_ref(&self) -> MultihashRef {
        MultihashRef { bytes: &self.bytes }
    }

    /// Returns which hashing algorithm is used in this multihash.
    #[inline]
    pub fn algorithm(&self) -> Hash {
        self.as_ref().algorithm()
    }

    /// Returns the hashed data.
    #[inline]
    pub fn digest(&self) -> &[u8] {
        self.as_ref().digest()
    }
}

impl<'a> PartialEq<MultihashRef<'a>> for Multihash {
    #[inline]
    fn eq(&self, other: &MultihashRef<'a>) -> bool {
        &*self.bytes == other.bytes
    }
}

/// Represents a valid multihash.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct MultihashRef<'a> {
    bytes: &'a [u8],
}

impl<'a> MultihashRef<'a> {
    /// Verifies whether `bytes` contains a valid multihash, and if so returns a `MultihashRef`.
    pub fn from_slice(input: &'a [u8]) -> Result<MultihashRef<'a>, DecodeError> {
        if input.is_empty() {
            return Err(DecodeError::BadInputLength);
        }

        // NOTE: We choose u16 here because there is no hashing algorithm implemented in this crate
        // whose length exceeds 2^16 - 1.
        let (code, bytes) = decode::u16(&input).map_err(|_| DecodeError::BadInputLength)?;

        let alg = Hash::from_code(code).ok_or(DecodeError::UnknownCode)?;
        let hash_len = alg.size() as usize;

        // Length of input after hash code should be exactly hash_len + 1
        if bytes.len() != hash_len + 1 {
            return Err(DecodeError::BadInputLength);
        }

        if bytes[0] as usize != hash_len {
            return Err(DecodeError::BadInputLength);
        }

        Ok(MultihashRef { bytes: input })
    }

    /// Returns which hashing algorithm is used in this multihash.
    #[inline]
    pub fn algorithm(&self) -> Hash {
        let (code, _) = decode::u16(&self.bytes).expect("multihash is known to be valid algorithm");
        Hash::from_code(code).expect("multihash is known to be valid")
    }

    /// Returns the hashed data.
    #[inline]
    pub fn digest(&self) -> &'a [u8] {
        let (_, bytes) = decode::u16(&self.bytes).expect("multihash is known to be valid digest");
        &bytes[1..]
    }

    /// Builds a `Multihash` that owns the data.
    ///
    /// This operation allocates.
    #[inline]
    pub fn into_owned(&self) -> Multihash {
        Multihash {
            bytes: self.bytes.to_owned(),
        }
    }

    /// Returns the bytes representation of this multihash.
    #[inline]
    pub fn as_bytes(&self) -> &'a [u8] {
        &self.bytes
    }
}

impl<'a> PartialEq<Multihash> for MultihashRef<'a> {
    #[inline]
    fn eq(&self, other: &Multihash) -> bool {
        self.bytes == &*other.bytes
    }
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
    use {Hash, Multihash};

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
                assert_eq!(hash, Multihash::from_bytes(hash.clone().into_bytes()).unwrap());
            }
        }
    }
}
