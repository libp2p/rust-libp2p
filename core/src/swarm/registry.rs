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

use crate::Multiaddr;
use smallvec::SmallVec;
use std::num::NonZeroUsize;

/// Hold a ranked collection of [`Multiaddr`] values.
///
/// Every address has an associated score and iterating over addresses will return them
/// in order from highest to lowest. When reaching the limit, addresses with the lowest
/// score will be dropped first.
#[derive(Debug, Clone)]
pub struct Addresses {
    /// Max. length of `registry`.
    limit: NonZeroUsize,
    /// The ranked sequence of addresses.
    registry: SmallVec<[Record; 8]>
}

// An address record associates a score to a Multiaddr.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Record {
    score: u32,
    addr: Multiaddr
}

impl Default for Addresses {
    fn default() -> Self {
        Addresses::new(NonZeroUsize::new(30).expect("30 > 0"))
    }
}

impl Addresses {
    /// Create a new address collection of bounded length.
    pub fn new(limit: NonZeroUsize) -> Self {
        Addresses {
            limit,
            registry: SmallVec::new()
        }
    }

    /// Add a [`Multiaddr`] to the collection.
    ///
    /// Adding an existing address is interpreted as additional
    /// confirmation and thus increases its score.
    pub fn add(&mut self, a: Multiaddr) {
        for r in &mut self.registry {
            if &r.addr == &a {
                r.score = r.score.saturating_add(1);
                isort(&mut self.registry);
                return ()
            }
        }
        if self.registry.len() == self.limit.get() {
            self.registry.pop();
        }
        let r = Record { score: 0, addr: a };
        self.registry.push(r)
    }

    /// Return an iterator over all [`Multiaddr`] values.
    ///
    /// The iteration is ordered by descending score.
    pub fn iter(&self) -> AddressIter<'_> {
        AddressIter { items: &self.registry, offset: 0 }
    }

    /// Return an iterator over all [`Multiaddr`] values.
    ///
    /// The iteration is ordered by descending score.
    pub fn into_iter(self) -> AddressIntoIter {
        AddressIntoIter { items: self.registry }
    }
}

/// An iterator over [`Multiaddr`] values.
#[derive(Clone)]
pub struct AddressIter<'a> {
    items: &'a [Record],
    offset: usize
}

impl<'a> Iterator for AddressIter<'a> {
    type Item = &'a Multiaddr;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset == self.items.len() {
            return None
        }
        let item = &self.items[self.offset];
        self.offset += 1;
        Some(&item.addr)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.items.len() - self.offset;
        (n, Some(n))
    }
}

impl<'a> ExactSizeIterator for AddressIter<'a> {}

/// An iterator over [`Multiaddr`] values.
#[derive(Clone)]
pub struct AddressIntoIter {
    items: SmallVec<[Record; 8]>,
}

impl Iterator for AddressIntoIter {
    type Item = Multiaddr;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.items.is_empty() {
            Some(self.items.remove(0).addr)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.items.len();
        (n, Some(n))
    }
}

impl ExactSizeIterator for AddressIntoIter {}

// Reverse insertion sort.
fn isort(xs: &mut [Record]) {
    for i in 1 .. xs.len() {
        for j in (1 ..= i).rev() {
            if xs[j].score <= xs[j - 1].score {
                break
            }
            xs.swap(j, j - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Multiaddr;
    use quickcheck::QuickCheck;
    use super::{isort, Record};

    #[test]
    fn isort_sorts() {
        fn property(xs: Vec<u32>) -> bool {
            let mut xs = xs.into_iter()
                .map(|s| Record { score: s, addr: Multiaddr::empty() })
                .collect::<Vec<_>>();

            isort(&mut xs);

            for i in 1 .. xs.len() {
                assert!(xs[i - 1].score >= xs[i].score)
            }

            true
        }
        QuickCheck::new().quickcheck(property as fn(Vec<u32>) -> bool)
    }
}
