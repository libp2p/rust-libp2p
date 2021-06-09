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

use crate::upgrade::{Role, Upgrade};

/// Upgrade that can be disabled at runtime.
///
/// Wraps around an `Option<T>` and makes it available or not depending on whether it contains or
/// not an upgrade.
#[derive(Debug, Clone)]
pub struct OptionalUpgrade<T>(Option<T>);

impl<T> OptionalUpgrade<T> {
    /// Creates an enabled `OptionalUpgrade`.
    pub fn some(inner: T) -> Self {
        OptionalUpgrade(Some(inner))
    }

    /// Creates a disabled `OptionalUpgrade`.
    pub fn none() -> Self {
        OptionalUpgrade(None)
    }
}

impl<T, C> Upgrade<C> for OptionalUpgrade<T>
where
    T: Upgrade<C>,
    <T::InfoIter as IntoIterator>::IntoIter: Send,
{
    type Info = T::Info;
    type InfoIter = Iter<<T::InfoIter as IntoIterator>::IntoIter>;
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn protocol_info(&self) -> Self::InfoIter {
        Iter(self.0.as_ref().map(|p| p.protocol_info().into_iter()))
    }

    fn upgrade(self, sock: C, info: Self::Info, role: Role) -> Self::Future {
        if let Some(inner) = self.0 {
            inner.upgrade(sock, info, role)
        } else {
            panic!("Bad API usage; a protocol has been negotiated while this struct contains None")
        }
    }
}

/// Iterator that flattens an `Option<T>` where `T` is an iterator.
#[derive(Debug, Clone)]
pub struct Iter<T>(Option<T>);

impl<T> Iterator for Iter<T>
where
    T: Iterator,
{
    type Item = T::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(iter) = self.0.as_mut() {
            iter.next()
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if let Some(iter) = self.0.as_ref() {
            iter.size_hint()
        } else {
            (0, Some(0))
        }
    }
}

impl<T> ExactSizeIterator for Iter<T>
where
    T: ExactSizeIterator
{
}
