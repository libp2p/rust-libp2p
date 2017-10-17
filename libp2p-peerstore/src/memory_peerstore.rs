use std::collections::{HashMap, HashSet};
use std::collections::hash_map;
use multiaddr::Multiaddr;
use peer::PeerId;
use peer_info::PeerInfo;
use super::TTL;
use peerstore::*;

pub struct MemoryPeerstore<T> {
	store: HashMap<PeerId, PeerInfo<T>>,
}

impl<T> MemoryPeerstore<T> {
	pub fn new() -> MemoryPeerstore<T> {
		MemoryPeerstore {
			store: HashMap::new(),
		}
	}
}

impl<T> Peerstore<T> for MemoryPeerstore<T> {
	fn add_peer(&mut self, peer_id: PeerId, peer_info: PeerInfo<T>) {
		self.store.insert(peer_id, peer_info);
	}
	/// Returns a list of peers in this Peerstore
	fn peers(&self) -> Vec<&PeerId> {
		// this is terrible but I honestly can't think of any other way than to hand off ownership
		// through this type of allocation or handing off the entire hashmap and letting people do what they
		// want with that
		self.store.keys().collect()
	}
	/// Returns the PeerInfo for a specific peer in this peer store, or None if it doesn't exist.
	fn peer_info(&self, peer_id: &PeerId) -> Option<&PeerInfo<T>> {
		self.store.get(peer_id)
	}

	/// Try to get a property for a given peer
	fn get_data(&self, peer_id: &PeerId, key: &str) -> Option<&T> {
		match self.store.get(peer_id) {
			None => None,
			Some(peer_info) => peer_info.get_data(key),
		}
	}
	/// Set a property for a given peer
	fn put_data(&mut self, peer_id: &PeerId, key: String, val: T) {
		match self.store.get_mut(peer_id) {
			None => (),
			Some(mut peer_info) => {
				peer_info.set_data(key, val);
			},
		}
	}

	/// Adds an address to a peer
	fn add_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, ttl: TTL) {
		match self.store.get_mut(peer_id) {
			None => (),
			Some(peer_info) => peer_info.add_addr(addr),
		}
	}

	// AddAddrs gives AddrManager addresses to use, with a given ttl
	// (time-to-live), after which the address is no longer valid.
	// If the manager has a longer TTL, the operation is a no-op for that address
	fn add_addrs(&mut self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: TTL) {
		match self.store.get_mut(peer_id) {
			None => (),
			Some(peer_info) => {
				for addr in addrs {
					peer_info.add_addr(addr)
				}
			},
		}
	}

	// SetAddr calls mgr.SetAddrs(p, addr, ttl)
	fn set_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, ttl: TTL) {
		self.set_addrs(peer_id, vec![addr], ttl)
	}

	// SetAddrs sets the ttl on addresses. This clears any TTL there previously.
	// This is used when we receive the best estimate of the validity of an address.
	fn set_addrs(&mut self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: TTL) {
		match self.store.get_mut(peer_id) {
			None => (),
			Some(peer_info) => peer_info.set_addrs(addrs),
		}
	}

	/// Returns all known (and valid) addresses for a given peer
	fn addrs(&self, peer_id: &PeerId) -> &[Multiaddr] {
		match self.store.get(peer_id) {
			None => &[],
			Some(peer_info) => peer_info.get_addrs(),
		}
	}

	/// Removes all previously stored addresses
	fn clear_addrs(&mut self, peer_id: &PeerId) {
		match self.store.get_mut(peer_id) {
			None => (),
			Some(peer_info) => peer_info.set_addrs(vec![]),
		}
	}

	/// Get public key for a peer
	fn get_pub_key(&self, peer_id: &PeerId) -> Option<&[u8]> {
		self.store.get(peer_id).map(|peer_info| peer_info.get_public_key())
	}

	/// Set public key for a peer
	fn set_pub_key(&mut self, peer_id: &PeerId, key: Vec<u8>) {
		self.store.get_mut(peer_id).map(|peer_info| peer_info.set_public_key(key));
	}
}

#[cfg(test)]
mod tests {
	use peer::PeerId;
	use super::{PeerInfo, Peerstore, MemoryPeerstore};

	#[test]
	fn insert_get_and_list() {
		let peer_id = PeerId::new(vec![1,2,3]);
		let peer_info = PeerInfo::new();
		let mut peer_store: MemoryPeerstore<u8> = MemoryPeerstore::new();
		peer_store.add_peer(peer_id.clone(), peer_info);
		peer_store.put_data(&peer_id, "test".into(), 123u8).unwrap();
		let got = peer_store.get_data(&peer_id, "test").expect("should be able to fetch");
		assert_eq!(*got, 123u8);
	}
}
