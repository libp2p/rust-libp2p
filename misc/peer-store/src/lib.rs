mod behaviour;
mod memory_store;
mod store;

use std::time::SystemTime;

pub use behaviour::{Behaviour, Event};
use libp2p_core::Multiaddr;
pub use store::Store;
pub use memory_store::MemoryStore;

pub struct AddressRecord<'a> {
    address: &'a Multiaddr,
    last_seen: &'a SystemTime,
}
impl<'a> AddressRecord<'a> {
    /// The address of this record.
    pub fn address(&self) -> &Multiaddr {
        self.address
    }
    /// How much time has passed since the address is last reported wrt. current time.  
    /// This may fail because of system time change.
    pub fn last_seen(&self) -> Result<std::time::Duration, std::time::SystemTimeError> {
        self.last_seen.elapsed()
    }
}
