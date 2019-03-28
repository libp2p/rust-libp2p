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

use multiaddr::Multiaddr;
use smallvec::SmallVec;
use std::{fmt, iter, slice};

/// A non-empty sequence of [`Multiaddr`]s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiaddrSeq {
    head: Multiaddr,
    tail: SmallVec<[Multiaddr; 4]>
}

impl fmt::Display for MultiaddrSeq {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl MultiaddrSeq {
    /// Create a one-element multi-address sequence.
    pub fn singleton(a: Multiaddr) -> Self {
        MultiaddrSeq { head: a, tail: SmallVec::new() }
    }

    /// Get the number of addresses in this sequence.
    pub fn len(&self) -> usize {
        1 + self.tail.len()
    }

    /// Get a reference to the first [`Multiaddr`] of this sequence.
    pub fn head(&self) -> &Multiaddr {
        &self.head
    }

    /// Get a reference to a [`Multiaddr`] by index.
    pub fn get(&self, pos: usize) -> Option<&Multiaddr> {
        if pos == 0 {
            return Some(&self.head)
        }
        self.tail.get(pos)
    }

    /// Get a unique reference to a [`Multiaddr`] by index.
    pub fn get_mut(&mut self, pos: usize) -> Option<&mut Multiaddr> {
        if pos == 0 {
            return Some(&mut self.head)
        }
        self.tail.get_mut(pos)
    }

    /// Add a [`Multiaddr`] to the end of this sequence.
    pub fn push(&mut self, a: Multiaddr) {
        self.tail.push(a)
    }

    /// Remove and return the last [`Multiaddr`] of this sequence.
    ///
    /// This removes all but the first [`Multiaddr`] from this sequence.
    pub fn pop(&mut self) -> Option<Multiaddr> {
        self.tail.pop()
    }

    /// Replace the first element with the given [`Multiaddr`].
    pub fn replace_head(&mut self, a: Multiaddr) {
        self.head = a
    }

    /// Create an [`Iterator`] over all [`Multiaddr`] of this sequence.
    pub fn iter(&self) -> MultiAddrIter {
        MultiAddrIter {
            inner: std::iter::once(&self.head).chain(self.tail.iter())
        }
    }

    /// Create a mutable [`Iterator`] over all [`Multiaddr`] of this sequence.
    pub fn iter_mut(&mut self) -> MultiAddrIterMut {
        MultiAddrIterMut {
            inner: std::iter::once(&mut self.head).chain(self.tail.iter_mut())
        }
    }
}

impl IntoIterator for MultiaddrSeq {
    type Item = Multiaddr;
    type IntoIter = IntoMultiAddrIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoMultiAddrIter {
            inner: std::iter::once(self.head).chain(self.tail.into_iter())
        }
    }
}

/// [`Iterator`] implementation returning references to each [`Multiaddr`] in order.
pub struct MultiAddrIter<'a> {
    inner: iter::Chain<iter::Once<&'a Multiaddr>, slice::Iter<'a, Multiaddr>>
}

impl<'a> iter::Iterator for MultiAddrIter<'a> {
    type Item = &'a Multiaddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// [`Iterator`] implementation returning unique references to each [`Multiaddr`] in order.
pub struct MultiAddrIterMut<'a> {
    inner: iter::Chain<iter::Once<&'a mut Multiaddr>, slice::IterMut<'a, Multiaddr>>
}

impl<'a> iter::Iterator for MultiAddrIterMut<'a> {
    type Item = &'a mut Multiaddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// [`Iterator`] implementation returning [`Multiaddr`]eses in order.
pub struct IntoMultiAddrIter {
    inner: iter::Chain<iter::Once<Multiaddr>, smallvec::IntoIter<[Multiaddr; 4]>>
}

impl iter::Iterator for IntoMultiAddrIter {
    type Item = Multiaddr;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

/// Convert from [`Multiaddr`] to [`MultiaddrSeq`].
impl From<Multiaddr> for MultiaddrSeq {
    fn from(x: Multiaddr) -> Self {
        MultiaddrSeq::singleton(x)
    }
}

