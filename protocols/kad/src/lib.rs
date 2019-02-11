// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Kademlia protocol. Allows peer discovery, records store and records fetch.
//!
//! # Usage
//!
//! Usage is done in the following steps:
//!
//! - Build a `KadSystemConfig` and a `KadConnecConfig` object that contain the way you want the
//!   Kademlia protocol to behave.
//!
//! - Create a swarm that upgrades incoming connections with the `KadConnecConfig`.
//!
//! - Build a `KadSystem` from the `KadSystemConfig`. This requires passing a closure that provides
//!   the Kademlia controller of a peer.
//!
//! - You can perform queries using the `KadSystem`.
//!

// TODO: we allow dead_code for now because this library contains a lot of unused code that will
//       be useful later for record store
#![allow(dead_code)]

// # Crate organization
//
// The crate contains three levels of abstractions over the Kademlia protocol.
//
// - The first level of abstraction is in `protocol`. The API of this module lets you turn a raw
//   bytes stream (`AsyncRead + AsyncWrite`) into a `Sink + Stream` of raw but strongly-typed
//   Kademlia messages.
//
// - The second level of abstraction is in `kad_server`. Its API lets you upgrade a connection and
//   obtain a future (that must be driven to completion), plus a controller. Processing the future
//   will automatically respond to Kad requests received by the remote. The controller lets you
//   send your own requests to this remote and obtain strongly-typed responses.
//
// - The third level of abstraction is in `high_level`. This module only provides the
//   `KademliaSystem`.
//

pub use self::kbucket::KBucketsPeerId;
pub use self::libp2p::{Kademlia, KademliaOut};
pub use self::libp2p::protocol::KadConnectionType;

pub mod kbucket;
pub mod parity;

mod addresses;
mod libp2p;
mod query;

use libp2p_core::PeerId;
use std::cmp::Ordering;

/// Generates a random `PeerId` that belongs to the given bucket.
///
/// Returns an error if `bucket_num` is out of range.
fn gen_random_id(my_id: &PeerId, bucket_num: usize) -> Result<PeerId, ()> {
    let my_id_len = my_id.as_bytes().len();

    // TODO: this 2 is magic here; it is the length of the hash of the multihash
    let bits_diff = bucket_num + 1;
    if bits_diff > 8 * (my_id_len - 2) {
        return Err(());
    }

    let mut random_id = [0; 64];
    for byte in 0..my_id_len {
        match byte.cmp(&(my_id_len - bits_diff / 8 - 1)) {
            Ordering::Less => {
                random_id[byte] = my_id.as_bytes()[byte];
            }
            Ordering::Equal => {
                let mask: u8 = (1 << (bits_diff % 8)) - 1;
                random_id[byte] = (my_id.as_bytes()[byte] & !mask) | (rand::random::<u8>() & mask);
            }
            Ordering::Greater => {
                random_id[byte] = rand::random();
            }
        }
    }

    let peer_id = PeerId::from_bytes(random_id[..my_id_len].to_owned())
        .expect("randomly-generated peer ID should always be valid");
    Ok(peer_id)
}
