use multiaddr::Multiaddr;
use peer::PeerId;
use peer_info::PeerInfo;
use super::TTL;

pub trait Peerstore<T> {
	/// Add a peer to this peer store
	fn add_peer(&mut self, peer_id: PeerId, peer_info: PeerInfo<T>);

	/// Returns a list of peers in this Peerstore
	fn peers(&self) -> Vec<&PeerId>;

	/// Returns the PeerInfo for a specific peer in this peer store, or None if it doesn't exist.
	fn peer_info(&self, peer_id: &PeerId) -> Option<&PeerInfo<T>>;

	/// Try to get a property for a given peer
	fn get_data(&self, peer_id: &PeerId, key: &str) -> Option<&T>;

	/// Set a property for a given peer
	fn put_data(&mut self, peer_id: &PeerId, key: String, val: T);

	/// Adds an address to a peer
	fn add_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, ttl: TTL);

	// AddAddrs gives AddrManager addresses to use, with a given ttl
	// (time-to-live), after which the address is no longer valid.
	// If the manager has a longer TTL, the operation is a no-op for that address
	fn add_addrs(&mut self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: TTL);

	// SetAddr calls mgr.SetAddrs(p, addr, ttl)
	fn set_addr(&mut self, peer_id: &PeerId, addr: Multiaddr, ttl: TTL);

	// SetAddrs sets the ttl on addresses. This clears any TTL there previously.
	// This is used when we receive the best estimate of the validity of an address.
	fn set_addrs(&mut self, peer_id: &PeerId, addrs: Vec<Multiaddr>, ttl: TTL);

	/// Returns all known (and valid) addresses for a given peer
	fn addrs(&self, peer_id: &PeerId) -> &[Multiaddr];

	/// Removes all previously stored addresses
	fn clear_addrs(&mut self, peer_id: &PeerId);

	/// Get public key for a peer
	fn get_pub_key(&self, peer_id: &PeerId) -> Option<&[u8]>;

	/// Set public key for a peer
	fn set_pub_key(&mut self, peer_id: &PeerId, key: Vec<u8>);
}
