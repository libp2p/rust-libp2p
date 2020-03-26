// Copyright 2020 Parity Technologies (UK) Ltd.
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
use std::sync::Arc;

/// MAX_INLINE is the maximum size of a multiaddr that can be stored inline.
/// There is an overhead of 2 bytes, 1 for the length and 1 for the enum discriminator.
/// 30 is chosen so that the overall size is 32. This should still be big enough to fit
/// a multiaddr containing an ipv4 or ipv6 address and port.
///
/// More complex multiaddrs like those containing peer ids will be stored on the heap.
const MAX_INLINE: usize = 30;

#[derive(Clone)]
pub(crate) enum Storage {
    /// hash is stored inline if it is smaller than MAX_INLINE
    Inline(u8, [u8; MAX_INLINE]),
    /// hash is stored on the heap. this must be only used if the hash is actually larger than
    /// MAX_INLINE bytes to ensure a unique representation.
    Heap(Arc<[u8]>),
}

impl Storage {
    /// The raw bytes.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Storage::Inline(len, bytes) => &bytes[..(*len as usize)],
            Storage::Heap(data) => &data,
        }
    }

    /// Creates storage from a slice.
    /// For a size up to MAX_INLINE, this will not allocate.
    pub fn from_slice(slice: &[u8]) -> Self {
        let len = slice.len();
        if len <= MAX_INLINE {
            let mut data: [u8; MAX_INLINE] = [0; MAX_INLINE];
            data[..len].copy_from_slice(slice);
            Storage::Inline(len as u8, data)
        } else {
            Storage::Heap(slice.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Multiaddr;
    use super::{Storage, MAX_INLINE};
    use quickcheck::quickcheck;

    #[test]
    fn multihash_size() {
        fn assert_size(ma: &str, n: usize, inline: bool) {
            let ma: Multiaddr = ma.parse().unwrap();
            assert_eq!(ma.as_ref().len(), n);
            assert_eq!(n <= MAX_INLINE, inline);
        }
        assert_size("/ip4/127.0.0.1", 5, true);
        assert_size("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000", 20, true);
        assert_size("/dns4/0123456789012345678901234/tcp/8000", 30, true);
        assert_size("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/ws/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC", 59, false);
    }

    #[test]
    fn struct_size() {
        // this should be true for both 32 and 64 bit archs
        assert_eq!(std::mem::size_of::<Storage>(), 32);
    }

    #[test]
    fn roundtrip() {
        // check that .bytes() returns whatever the storage was created with
        for i in 0..((MAX_INLINE + 10) as u8) {
            let data = (0..i).collect::<Vec<u8>>();
            let storage = Storage::from_slice(&data);
            assert_eq!(data, storage.bytes());
        }
    }

    fn check_invariants(storage: Storage) -> bool {
        match storage {
            Storage::Inline(len, _) => len as usize <= MAX_INLINE,
            Storage::Heap(arc) => arc.len() > MAX_INLINE,
        }
    }

    quickcheck! {
        fn roundtrip_check(data: Vec<u8>) -> bool {
            let storage = Storage::from_slice(&data);
            storage.bytes() == data.as_slice() && check_invariants(storage)
        }
    }
}
