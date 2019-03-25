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

//! Docs

use crate::kbucket::KBucketsPeerId;
use libp2p_core::PeerId;
use multihash::Multihash;
use std::num::NonZeroUsize;

///Hi
#[derive(Clone, Debug, PartialEq)]
pub struct KadHash {
    peer_id: PeerId,
    hash: Multihash,
}

impl KadHash {
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn hash(&self) -> &Multihash {
        &self.hash
    }
}

impl From<&PeerId> for KadHash {
    fn from(peer_id: &PeerId) -> Self {
        KadHash{peer_id: peer_id.clone(), hash: multihash::encode(multihash::Hash::SHA2256, peer_id.as_bytes()).expect("sha2-256 is always supported")}
    }
}

impl KBucketsPeerId for KadHash {
    fn distance_with(&self, other: &Self) -> u32 {
//        self.hash().distance_with(other.hash())
        <Multihash as KBucketsPeerId<Multihash>>::distance_with(self.hash(), other.hash())
    }

    fn max_distance() -> NonZeroUsize {
        <Multihash as KBucketsPeerId>::max_distance()
    }
}

impl PartialEq<multihash::Multihash> for KadHash {
    #[inline]
    fn eq(&self, other: &multihash::Multihash) -> bool {
        self.hash() == other
    }
}

impl PartialEq<KadHash> for multihash::Multihash {
    #[inline]
    fn eq(&self, other: &KadHash) -> bool {
        self == other.hash()
    }
}
