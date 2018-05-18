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

//! The `peerstore` crate allows one to store information about a peer.
//!
//! `peerstore` is a key-value database, where the keys are multihashes (which usually corresponds
//! to the hash of the public key of the peer, but that is not enforced by this crate) and the
//! values are the public key and a list of multiaddresses. Additionally, the multiaddresses stored
//! by the `peerstore` have a time-to-live after which they disappear.
//!
//! This crate consists of a generic `Peerstore` trait and the follow implementations:
//!
//! - `JsonPeerstore`: Stores the information in a single JSON file.
//! - `MemoryPeerstore`: Stores the information in memory.
//!
//! Note that the peerstore implementations do not consider information inside a peer store to be
//! critical. In case of an error (eg. corrupted file, disk error, etc.) they will prefer to lose
//! data rather than returning the error.
//!
//! # Example
//!
//! ```
//! extern crate multiaddr;
//! extern crate libp2p_peerstore;
//!
//! # fn main() {
//! use libp2p_peerstore::memory_peerstore::MemoryPeerstore;
//! use libp2p_peerstore::{PeerId, Peerstore, PeerAccess};
//! use multiaddr::Multiaddr;
//! use std::time::Duration;
//!
//! // In this example we use a `MemoryPeerstore`, but you can easily swap it for another backend.
//! let mut peerstore = MemoryPeerstore::empty();
//! let peer_id = PeerId::from_public_key(&[1, 2, 3, 4]);
//!
//! // Let's write some information about a peer.
//! {
//!     // `peer_or_create` mutably borrows the peerstore, so we have to do it in a local scope.
//!     let mut peer = peerstore.peer_or_create(&peer_id);
//!     peer.add_addr("/ip4/10.11.12.13/tcp/20000".parse::<Multiaddr>().unwrap(),
//!                   Duration::from_millis(5000));
//! }
//!
//! // Now let's load back the info.
//! {
//!     let mut peer = peerstore.peer(&peer_id).expect("peer doesn't exist in the peerstore");
//!     assert_eq!(peer.addrs().collect::<Vec<_>>(),
//!                &["/ip4/10.11.12.13/tcp/20000".parse::<Multiaddr>().unwrap()]);
//! }
//! # }
//! ```

extern crate bs58;
extern crate datastore;
extern crate futures;
extern crate multiaddr;
extern crate multihash;
extern crate owning_ref;
#[macro_use]
extern crate quick_error;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::{fmt, str::FromStr};

pub use self::peerstore::{PeerAccess, Peerstore};

#[macro_use]
mod peerstore_tests;

pub mod json_peerstore;
pub mod memory_peerstore;
mod peer_info;
mod peerstore;

pub type TTL = std::time::Duration;

/// Identifier of a peer of the network.
///
/// The data is a multihash of the public key of the peer.
// TODO: maybe keep things in decoded version?
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PeerId {
    multihash: Vec<u8>,
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PeerId({})", self.to_base58())
    }
}

impl PeerId {
    /// Builds a `PeerId` from a public key.
    #[inline]
    pub fn from_public_key(public_key: &[u8]) -> PeerId {
        let data = multihash::encode(multihash::Hash::SHA2256, public_key)
            .expect("sha2-256 is always supported");
        PeerId { multihash: data }
    }

    /// Checks whether `data` is a valid `PeerId`. If so, returns the `PeerId`. If not, returns
    /// back the data as an error.
    #[inline]
    pub fn from_bytes(data: Vec<u8>) -> Result<PeerId, Vec<u8>> {
        match multihash::decode(&data) {
            Ok(_) => Ok(PeerId { multihash: data }),
            Err(_) => Err(data),
        }
    }

    /// Returns a raw bytes representation of this `PeerId`.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.multihash
    }

    /// Returns a raw bytes representation of this `PeerId`.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.multihash
    }

    /// Returns a base-58 encoded string of this `PeerId`.
    #[inline]
    pub fn to_base58(&self) -> String {
        bs58::encode(&self.multihash).into_string()
    }

    /// Returns the raw bytes of the hash of this `PeerId`.
    #[inline]
    pub fn hash(&self) -> &[u8] {
        multihash::decode(&self.multihash)
            .expect("our inner value should always be valid")
            .digest
    }

    /// Checks whether the public key passed as parameter matches the public key of this `PeerId`.
    ///
    /// Returns `None` if this `PeerId`s hash algorithm is not supported when encoding the
    /// given public key, otherwise `Some` boolean as the result of an equality check.
    pub fn is_public_key(&self, public_key: &[u8]) -> Option<bool> {
        let alg = multihash::decode(&self.multihash)
            .expect("our inner value should always be valid")
            .alg;
        match multihash::encode(alg, public_key) {
            Ok(compare) => Some(compare == self.multihash),
            Err(multihash::Error::UnsupportedType) => None,
            Err(_) => Some(false),
        }
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum ParseError {
        B58(e: bs58::decode::DecodeError) {
            display("base-58 decode error: {}", e)
            cause(e)
            from()
        }
        MultiHash {
            display("decoding multihash failed")
        }
    }
}

impl FromStr for PeerId {
    type Err = ParseError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = bs58::decode(s).into_vec()?;
        PeerId::from_bytes(bytes).map_err(|_| ParseError::MultiHash)
    }
}

