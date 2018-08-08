/// ! # multihash
/// !
/// ! Implementation of [multihash](https://github.com/multiformats/multihash)
/// ! in Rust.
/// Representation of a Multiaddr.

extern crate sha1;
extern crate sha2;
extern crate tiny_keccak;

mod errors;
mod hashes;

use std::fmt::Write;
use sha2::Digest;
use tiny_keccak::Keccak;

pub use hashes::Hash;
pub use errors::Error;

// Helper macro for encoding input into output using sha1, sha2 or tiny_keccak
macro_rules! encode {
    (sha1, Sha1, $input:expr, $output:expr) => ({
        let mut hasher = sha1::Sha1::new();
        hasher.update($input);
        $output.copy_from_slice(&hasher.digest().bytes());
    });
    (sha2, $algorithm:ident, $input:expr, $output:expr) => ({
        let mut hasher = sha2::$algorithm::default();
        hasher.input($input);
        $output.copy_from_slice(hasher.result().as_ref());
    });
    (tiny, $constructor:ident, $input:expr, $output:expr) => ({
        let mut kec = Keccak::$constructor();
        kec.update($input);
        kec.finalize($output);
    });
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

            _ => return Err(Error::UnsupportedType)
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
pub fn encode(hash: Hash, input: &[u8]) -> Result<Multihash, Error> {
    let size = hash.size();
    let mut output = Vec::new();
    output.resize(2 + size as usize, 0);
    output[0] = hash.code();
    output[1] = size;

    match_encoder!(hash for (input, &mut output[2..]) {
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
    });

    Ok(Multihash { bytes: output })
}

/// Represents a valid multihash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Multihash {
    bytes: Vec<u8>
}

impl Multihash {
    /// Verifies whether `bytes` contains a valid multihash, and if so returns a `Multihash`.
    #[inline]
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Multihash, Error> {
        if let Err(err) = MultihashRef::from_slice(&bytes) {
            return Err(err);
        }

        Ok(Multihash { bytes })
    }

    /// Returns the bytes representation of the multihash.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
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
    pub fn hash_data(&self) -> &[u8] {
        self.as_ref().hash_data()
    }
}

/// Represents a valid multihash.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MultihashRef<'a> {
    bytes: &'a [u8]
}

impl<'a> MultihashRef<'a> {
    /// Verifies whether `bytes` contains a valid multihash, and if so returns a `MultihashRef`.
    pub fn from_slice(input: &'a [u8]) -> Result<MultihashRef<'a>, Error> {
        if input.is_empty() {
            return Err(Error::BadInputLength);
        }

        let code = input[0];

        let alg = Hash::from_code(code)?;
        let hash_len = alg.size() as usize;

        // length of input should be exactly hash_len + 2
        if input.len() != hash_len + 2 {
            return Err(Error::BadInputLength);
        }

        if input[1] as usize != hash_len {
            return Err(Error::BadInputLength);
        }

        Ok(MultihashRef { bytes: input })
    }

    /// Returns which hashing algorithm is used in this multihash.
    #[inline]
    pub fn algorithm(&self) -> Hash {
        Hash::from_code(self.bytes[0]).expect("multihash is known to be valid")
    }

    /// Returns the hashed data.
    #[inline]
    pub fn hash_data(&self) -> &'a [u8] {
        &self.bytes[2..]
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
