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

//! The objective of the `peerstore` crate is to provide a key-value storage. Keys are peer IDs,
//! and the `PeerInfo` struct in this module is the value.
//!
//! Note that the `PeerInfo` struct implements `PartialOrd` so that it can be stored in a
//! `Datastore`. This operation currently simply compares the public keys of the `PeerInfo`s.
//! If the `PeerInfo` struct ever gets exposed to the public API of the crate, we may want to give
//! more thoughts about this.

use TTL;
use multiaddr::Multiaddr;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use serde::de::Error as DeserializerError;
use serde::ser::SerializeStruct;
use std::cmp::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Information about a peer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerInfo {
	// Adresses, and the time at which they will be considered expired.
	addrs: Vec<(Multiaddr, SystemTime)>,
	public_key: Option<Vec<u8>>,
}

impl PeerInfo {
	/// Builds a new empty `PeerInfo`.
	#[inline]
	pub fn new() -> PeerInfo {
		PeerInfo { addrs: vec![], public_key: None }
	}

	/// Returns the list of the non-expired addresses stored in this `PeerInfo`.
	///
	/// > **Note**: Keep in mind that this function is racy because addresses can expire between
	/// >   		the moment when you get them and the moment when you process them.
	// TODO: use -> impl Iterator eventually
	#[inline]
	pub fn addrs<'a>(&'a self) -> Box<Iterator<Item = &'a Multiaddr> + 'a> {
		let now = SystemTime::now();
		Box::new(self.addrs.iter().filter_map(move |&(ref addr, ref expires)| if *expires >= now {
			Some(addr)
		} else {
			None
		}))
	}

	/// Sets the list of addresses and their time-to-live.
	///
	/// This removes all previously-stored addresses.
	#[inline]
	pub fn set_addrs<I>(&mut self, addrs: I)
		where I: IntoIterator<Item = (Multiaddr, TTL)>
	{
		let now = SystemTime::now();
		self.addrs = addrs.into_iter().map(move |(addr, ttl)| (addr, now + ttl)).collect();
	}

	/// Adds a single address and its time-to-live.
	///
	/// If the peer info already knows about that address but with a longer TTL, then the operation
	/// is a no-op.
	pub fn add_addr(&mut self, addr: Multiaddr, ttl: TTL, behaviour: AddAddrBehaviour) {
		let expires = SystemTime::now() + ttl;

		if let Some(&mut (_, ref mut existing_expires)) =
			self.addrs.iter_mut().find(|&&mut (ref a, _)| a == &addr)
		{
			if behaviour == AddAddrBehaviour::OverwriteTtl || *existing_expires < expires {
				*existing_expires = expires;
			}
			return;
		}

		self.addrs.push((addr, expires));
	}

	/// Sets the public key stored in this `PeerInfo`.
	#[inline]
	pub fn set_public_key(&mut self, key: Vec<u8>) {
		self.public_key = Some(key);
	}

	/// Returns the public key stored in this `PeerInfo`, if any.
	#[inline]
	pub fn public_key(&self) -> Option<&[u8]> {
		self.public_key.as_ref().map(|k| &**k)
	}
}

/// Behaviour of the `add_addr` function.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AddAddrBehaviour {
	/// Always overwrite the existing TTL.
	OverwriteTtl,
	/// Don't overwrite if the TTL is larger.
	IgnoreTtlIfInferior,
}

impl Serialize for PeerInfo {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where S: Serializer
	{
		let mut s = serializer.serialize_struct("PeerInfo", 2)?;
		s.serialize_field(
			"addrs",
			&self.addrs
			     .iter()
			     .map(|&(ref addr, ref expires)| {
				let addr = addr.to_bytes();
				let from_epoch = expires.duration_since(UNIX_EPOCH)
					// This `unwrap_or` case happens if the user has their system time set to
					// before EPOCH. Times-to-live will be be longer than expected, but it's a very
					// improbable corner case and is not attackable in any way, so we don't really
					// care.
				    .unwrap_or(Duration::new(0, 0));
				let secs = from_epoch.as_secs()
				                     .saturating_mul(1_000)
				                     .saturating_add(from_epoch.subsec_nanos() as u64 / 1_000_000);
				(addr, secs)
			})
			     .collect::<Vec<_>>(),
		)?;
		s.serialize_field("public_key", &self.public_key)?;
		s.end()
	}
}

impl<'de> Deserialize<'de> for PeerInfo {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where D: Deserializer<'de>
	{
		// We deserialize to an intermdiate struct first, then turn that struct into a `PeerInfo`.
		let interm = {
			#[derive(Deserialize)]
			struct Interm {
				addrs: Vec<(String, u64)>,
				public_key: Option<Vec<u8>>,
			}
			Interm::deserialize(deserializer)?
		};

		let addrs = {
			let mut out = Vec::with_capacity(interm.addrs.len());
			for (addr, since_epoch) in interm.addrs {
				let addr = match addr.parse::<Multiaddr>() {
					Ok(a) => a,
					Err(err) => return Err(DeserializerError::custom(err)),
				};
				let expires = UNIX_EPOCH + Duration::from_millis(since_epoch);
				out.push((addr, expires));
			}
			out
		};

		Ok(PeerInfo {
			addrs: addrs,
			public_key: interm.public_key,
		})
	}
}

impl PartialOrd for PeerInfo {
	#[inline]
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		// See module-level comment.
		match (&self.public_key, &other.public_key) {
			(&Some(ref my_pub), &Some(ref other_pub)) => {
				Some(my_pub.cmp(other_pub))
			}
			_ => {
				None
			}
		}
	}
}
