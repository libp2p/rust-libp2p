// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! Inside a KBucketsTable we would like to store the hash of a PeerId
//! even if a PeerId is itself already a hash. When querying the table
//! we may be interested in getting the PeerId back. This module provides
//! a struct, KadHash that stores together a PeerId and its hash for
//! convenience.

use arrayref::array_ref;
use libp2p_core::PeerId;

/// Used as key in a KBucketsTable for Kademlia. Stores the hash of a
/// PeerId, and the PeerId itself because it may need to be queried.
#[derive(Clone, Debug, PartialEq)]
pub struct KadHash {
    peer_id: PeerId,
    hash: [u8; 32],
}

/// Provide convenience getters.
impl KadHash {
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn hash(&self) -> &[u8; 32] {
        &self.hash
    }
}

impl From<PeerId> for KadHash {
    fn from(peer_id: PeerId) -> Self {
        let encoding = multihash::encode(multihash::Hash::SHA2256, peer_id.as_bytes()).expect("sha2-256 is always supported");

        KadHash{
            peer_id: peer_id,
            hash: array_ref!(encoding.digest(), 0, 32).clone(),
        }
    }
}

impl PartialEq<multihash::Multihash> for KadHash {
    #[inline]
    fn eq(&self, other: &multihash::Multihash) -> bool {
        self.hash() == other.digest()
    }
}


impl PartialEq<KadHash> for multihash::Multihash {
    #[inline]
    fn eq(&self, other: &KadHash) -> bool {
        self.digest() == other.hash()
    }
}
