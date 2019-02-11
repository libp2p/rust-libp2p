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

mod behaviour;
mod view;

pub use schnorrkel::{Keypair, PublicKey};
pub use self::behaviour::{Kademlia, KademliaOut};
pub mod protocol;

use crate::kbucket::KBucketsPeerId;

/// Namespace inside the DHT.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Namespace([u8; 4]);

impl From<[u8; 1]> for Namespace {
    fn from(bytes: [u8; 1]) -> Namespace {
        Namespace([bytes[0], 0, 0, 0])
    }
}

impl From<[u8; 2]> for Namespace {
    fn from(bytes: [u8; 2]) -> Namespace {
        Namespace([bytes[0], bytes[1], 0, 0])
    }
}

impl From<[u8; 3]> for Namespace {
    fn from(bytes: [u8; 3]) -> Namespace {
        Namespace([bytes[0], bytes[1], bytes[2], 0])
    }
}

impl From<[u8; 4]> for Namespace {
    fn from(bytes: [u8; 4]) -> Namespace {
        Namespace(bytes)
    }
}

impl KBucketsPeerId for Namespace {
    #[inline]
    fn distance_with(&self, other: &Self) -> u32 {
        let my_val = <byteorder::BigEndian as byteorder::ByteOrder>::read_u32(&self.0);
        let other_val = <byteorder::BigEndian as byteorder::ByteOrder>::read_u32(&other.0);
        let xor = my_val ^ other_val;
        32 - xor.leading_zeros()
    }

    #[inline]
    fn max_distance() -> usize {
        32
    }
}
