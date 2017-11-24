// Copyright 2017 Parity Technologies (UK) Ltd.
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

extern crate hex;
extern crate byteorder;

use std::fmt;
use std::collections::HashSet;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use hex::FromHex;

const ID: u64 = 0x00;
const SHA1: u64 = 0x11;
const SHA2_256: u64 = 0x12;
const SHA2_512: u64 = 0x13;
const SHA3_224: u64 = 0x17;
const SHA3_256: u64 = 0x16;
const SHA3_384: u64 = 0x15;
const SHA3_512: u64 = 0x14;
const SHA3: u64 = SHA3_512;
const KECCAK_224: u64 = 0x1A;
const KECCAK_256: u64 = 0x1B;
const KECCAK_384: u64 = 0x1C;
const KECCAK_512: u64 = 0x1D;
const SHAKE_128: u64 = 0x18;
const SHAKE_256: u64 = 0x19;
const DBL_SHA2_256: u64 = 0x56;
const MURMUR3: u64 = 0x22;

const BLAKE2B_MIN: u64 = 0xb201;
const BLAKE2B_MAX: u64 = 0xb240;
const BLAKE2S_MIN: u64 = 0xb241;
const BLAKE2S_MAX: u64 = 0xb260;

thread_local! {
	// Create a static set of CODES that we are willing to accept
	static CODES: HashSet<u64> = {
		let mut set = HashSet::new();
		set.insert(ID);
		set.insert(SHA1);
		set.insert(SHA2_256);
		set.insert(SHA2_512);
		set.insert(SHA3_224);
		set.insert(SHA3_256);
		set.insert(SHA3_384);
		set.insert(SHA3_512);
		set.insert(SHA3);
		set.insert(KECCAK_224);
		set.insert(KECCAK_256);
		set.insert(KECCAK_384);
		set.insert(KECCAK_512);
		set.insert(SHAKE_128);
		set.insert(SHAKE_256);
		set.insert(DBL_SHA2_256);
		set.insert(MURMUR3);
		for c in BLAKE2B_MIN..BLAKE2B_MAX { set.insert(c); }
		for c in BLAKE2S_MIN..BLAKE2S_MAX { set.insert(c); }
		set
	};
}

/// Error type for Multihash represents all possible decoding errors from hex string or bytes
#[derive(Debug)]
pub enum Error {
	/// Got invalid input hex
	FromHexError,
	/// Couldn't parse the code from bytes
	UnreadableCode,
	/// The code parsed (given in argument) isn't a known code for any hash
	UnknownCode(u64),
	/// Couldn't parse the length from bytes
	UnreadableLength,
	/// The length provided (found in first argument) doesn't match the actual
	/// length of the input digest (given in second argument).
	InvalidLength(u64, usize),
}

/// Multihash is a structure for tagging a hash with some meta data,
/// such as its type and length. This facilitiates reading them over
/// (for instance) a network stream. The format is as follows:
/// <hash function code><digest size><hash function output>
/// See the spec for more information.
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Multihash {
	/// The code of the hash algorithm in question
	code: u64,
	/// Length of the digest byte vector
	length: u64,
	/// Raw bytes of the hash digest
	digest: Vec<u8>,
}

/// Display of a Multihash prints the "raw" hex string, including code and length
impl fmt::Display for Multihash {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{:016x}{:016x}", self.code, self.length)?;
		for x in &self.digest {
			write!(f, "{:02x}", x)?;
		}
		Ok(())
	}
}

/// FromHex implementation is provided along with `Multihash::decode_hex_string` in case
/// someone already has FromHex imported and they would prefer to use that for some reason
impl FromHex for Multihash {
	type Error = Error;
	fn from_hex<T: AsRef<[u8]>>(s: T) -> Result<Self, Self::Error> {
		let bs = Vec::from_hex(s).map_err(|_| Error::FromHexError)?;
		Self::decode_bytes(bs)
	}
}

impl Multihash {
	/// Returns a byte vec representing the encoded multihash
	pub fn to_bytes(&self) -> Vec<u8> {
		let mut v = Vec::new();
		v.write_u64::<BigEndian>(self.code).expect("writing to a locally owned vec should never yield I/O errors");
		v.write_u64::<BigEndian>(self.length).expect("writing to a locally owned vec should never yield I/O errors");
		v.extend_from_slice(&self.digest);
		v
	}

	/// Takes a code for a digest type and a vec of bytes of the digest and returns a Multihash
	pub fn encode_bytes(code: u64, bytes: Vec<u8>) -> Result<Multihash, Error> {
		if !Multihash::valid_code(&code) {
			return Err(Error::UnknownCode(code));
		}
		Ok(Multihash {
			code: code,
			length: bytes.len() as u64,
			digest: bytes
		})
	}

	/// Takes a code for a digest type and a hex string of the digest and returns a Multihash
	pub fn encode_hex_string(code: u64, hex: String) -> Result<Multihash, Error> {
		let bytes = Vec::from_hex(&hex).map_err(|_| Error::FromHexError)?;
		Self::encode_bytes(code, bytes)
	}

	/// Decode a vec of bytes into a Multihash, consuming the vec in the process
	pub fn decode_bytes(bytes: Vec<u8>) -> Result<Multihash, Error> {
		let mut rdr = bytes.as_slice();
		let code = rdr.read_u64::<BigEndian>().map_err(|_| Error::UnreadableCode)?;
		let length = rdr.read_u64::<BigEndian>().map_err(|_| Error::UnreadableLength)?;
		let bytes = rdr.to_vec();

		if !Multihash::valid_code(&code) {
			return Err(Error::UnknownCode(code));
		}
		if bytes.len() != length as usize {
			return Err(Error::InvalidLength(length, bytes.len()));
		}

		Ok(Multihash {
			code: code,
			length: length,
			digest: bytes
		})
	}

	/// Convert a hex string to a Multihash
	pub fn decode_hex_string<T: AsRef<[u8]>>(s: T) -> Result<Multihash, Error> {
		/// Using this here to not have to reexport the hex::FromHex trait
		Multihash::from_hex(s)
	}

	/// Checks if the given code is a valid code for multihash
	pub fn valid_code(c: &u64) -> bool {
		CODES.with(|codes| codes.contains(c))
	}
}


#[cfg(test)]
mod tests {
	use super::{Multihash};

    #[test]
    fn multihash_can_be_displayed() {
		let hash = Multihash { code: 0x11, length: 3, digest: vec![42, 0, 255]};
		assert_eq!(format!("{}", hash), "000000000000001100000000000000032a00ff");
    }

	#[test]
	fn produces_correct_vec_of_bytes() {
		let digest_bytes = vec![44, 38, 180, 107, 104, 255, 198, 143, 249, 155, 69, 60, 29, 48, 65, 52, 19, 66, 45, 112, 100, 131, 191, 160, 249, 138, 94, 136, 98, 102, 231, 174];
		let hash = Multihash {
			code: 0x00,
			length: digest_bytes.len() as u64,
			digest: digest_bytes
		};
		let expected_bytes = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 44, 38, 180, 107, 104, 255, 198, 143, 249, 155, 69, 60, 29, 48, 65, 52, 19, 66, 45, 112, 100, 131, 191, 160, 249, 138, 94, 136, 98, 102, 231, 174];

		assert_eq!(hash.to_bytes(), expected_bytes);

		assert_eq!(Multihash::decode_bytes(expected_bytes).unwrap(), hash);
	}

	#[test]
	fn encoding_hashes() {
		let hex_string = "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";
		let hash = Multihash::encode_hex_string(0x00, hex_string.to_owned()).unwrap();
		let expected_bytes = vec![44,38,180,107,104,255,198,143,249,155,69,60,29,48,65,52,19,66,45,112,100,131,191,160,249,138,94,136,98,102,231,174];
		assert_eq!(hash, Multihash {
			code: 0x00,
			length: expected_bytes.len() as u64,
			digest: expected_bytes
		});
	}

    #[test]
    fn decoding_hashes() {
		let hex_string = "000000000000000000000000000000202c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";
		let hash = Multihash::decode_hex_string(hex_string).unwrap();
		let expected_bytes = vec![44,38,180,107,104,255,198,143,249,155,69,60,29,48,65,52,19,66,45,112,100,131,191,160,249,138,94,136,98,102,231,174];
		assert_eq!(hash, Multihash {
			code: 0x00,
			length: expected_bytes.len() as u64,
			digest: expected_bytes
		});
    }
}
