extern crate base58;

use std::fmt;
use base58::ToBase58;

/// A PeerId is a reference to a multihash
/// Ideally we would want to store the Multihash object directly here but because
/// the multihash package is lacking some things right now, lets store a reference to
/// some bytes that represent the full bytes of the multihash
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct PeerId {
	/// Rereference to multihash bytes
	multihash: Vec<u8>
}

impl fmt::Display for PeerId {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}", self.to_base58())
	}
}

impl PeerId {
	/// Create a new PeerId from a multihash
	pub fn new(mh: Vec<u8>) -> PeerId {
		PeerId { multihash: mh }
	}

	/// Outputs the multihash as a Base58 string,
	/// this is what we use as our stringified version of the ID
	pub fn to_base58(&self) -> String {
		self.multihash.to_base58()
	}
}


#[cfg(test)]
mod tests {
	extern crate multihash;
	use self::multihash::{encode, Hash};
	use super::{PeerId};

	#[test]
	fn peer_id_produces_correct_b58() {
		let multihash_bytes = encode(Hash::SHA2256, b"hello world").unwrap();
		let peer_id = PeerId::new(multihash_bytes);
		assert_eq!(peer_id.to_base58(), "QmaozNR7DZHQK1ZcU9p7QdrshMvXqWK6gpu5rmrkPdT3L4");
	}

	#[test]
	fn string_key_concatenates_correctly() {
		let multihash_bytes = encode(Hash::SHA2256, b"hello world").unwrap();
		let peer_id = PeerId::new(multihash_bytes);
		assert_eq!(peer_id.string_key("hello"), "QmaozNR7DZHQK1ZcU9p7QdrshMvXqWK6gpu5rmrkPdT3L4/hello");
	}
}
