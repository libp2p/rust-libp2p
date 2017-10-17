use std::time;
use std::collections::{HashMap, HashSet};
use multiaddr::Multiaddr;

pub struct PeerInfo<T> {
	public_key: Vec<u8>,
	addrs: Vec<Multiaddr>,
	data: HashMap<String, T>,
}

impl<T> PeerInfo<T> {
	pub fn new() -> PeerInfo<T> {
		PeerInfo {
			public_key: vec![],
			addrs: vec![],
			data: HashMap::new(),
		}
	}
	pub fn get_public_key(&self) -> &[u8] {
		&self.public_key
	}
	pub fn set_public_key(&mut self, key: Vec<u8>) {
		self.public_key = key;
	}
	pub fn get_addrs(&self) -> &[Multiaddr] {
		&self.addrs
	}
	pub fn set_addrs(&mut self, addrs: Vec<Multiaddr>) {
		self.addrs = addrs;
	}
	pub fn add_addr(&mut self, addr: Multiaddr) {
		self.addrs.push(addr); // TODO: This is stupid, a more advanced thing using TTLs need to be implemented
		self.addrs.dedup();
	}
	pub fn get_data(&self, key: &str) -> Option<&T> {
		self.data.get(key)
	}
	pub fn set_data(&mut self, key: String, val: T) -> Option<T> {
		self.data.insert(key, val)
	}
}
