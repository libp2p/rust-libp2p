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

use bytes::Bytes;
use upgrade::{ConnectionUpgrade, Endpoint};

/// Wraps around a `ConnectionUpgrade` and makes it possible to filter the protocols you want.
///
/// Only the protocol names for which `filter` returns `true` will be negotiable.
#[inline]
pub fn filter<U, F>(upgrade: U, filter: F) -> Filter<U, F>
where F: FnMut(&Bytes) -> bool + Clone,
{
    Filter {
        inner: upgrade,
        filter,
    }
}

/// See `upgrade::filter`.
#[derive(Debug, Copy, Clone)]
pub struct Filter<U, F> {
    inner: U,
    filter: F,
}

impl<C, U, F, Maf> ConnectionUpgrade<C, Maf> for Filter<U, F>
where
    U: ConnectionUpgrade<C, Maf>,
    F: FnMut(&Bytes) -> bool + Clone,
{
    type NamesIter = FilterIter<U::NamesIter, F>;
    type UpgradeIdentifier = U::UpgradeIdentifier;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        FilterIter {
            inner: self.inner.protocol_names(),
            filter: self.filter.clone(),
        }
    }

    type Output = U::Output;
    type MultiaddrFuture = U::MultiaddrFuture;
    type Future = U::Future;

    #[inline]
    fn upgrade(
        self,
        socket: C,
        id: Self::UpgradeIdentifier,
        ty: Endpoint,
        remote_addr: Maf,
    ) -> Self::Future {
        self.inner.upgrade(socket, id, ty, remote_addr)
    }
}

/// Iterator that allows filter protocols.
#[derive(Debug, Clone)]
pub struct FilterIter<I, F> {
    inner: I,
    filter: F,
}

impl<I, F, Id> Iterator for FilterIter<I, F>
where I: Iterator<Item = (Bytes, Id)>,
      F: FnMut(&Bytes) -> bool + Clone,
{
    type Item = I::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let val = match self.inner.next() {
                Some(out) => out,
                None => return None,
            };

            let filter = &mut self.filter;
            if filter(&val.0) {
                return Some(val);
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (_, max) = self.inner.size_hint();
        (0, max)
    }
}
