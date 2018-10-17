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

use futures::future;
use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

/// Wraps around a `ConnectionUpgrade` and makes it possible to enable or disable an upgrade.
#[inline]
pub fn toggleable<U>(upgrade: U) -> Toggleable<U> {
    Toggleable {
        inner: upgrade,
        enabled: true,
    }
}

/// See `upgrade::toggleable`.
#[derive(Debug, Copy, Clone)]
pub struct Toggleable<U> {
    inner: U,
    enabled: bool,
}

impl<U> Toggleable<U> {
    /// Toggles the upgrade.
    #[inline]
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    /// Returns true if the upgrade is enabled.
    #[inline]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Enables the upgrade.
    #[inline]
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disables the upgrade.
    #[inline]
    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

impl<C, U> ConnectionUpgrade<C> for Toggleable<U>
where
    C: AsyncRead + AsyncWrite,
    U: ConnectionUpgrade<C>,
{
    type NamesIter = ToggleableIter<U::NamesIter>;
    type UpgradeIdentifier = U::UpgradeIdentifier;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        ToggleableIter {
            inner: self.inner.protocol_names(),
            enabled: self.enabled,
        }
    }

    type Output = U::Output;
    type Future = future::Either<future::Empty<U::Output, IoError>, U::Future>;

    #[inline]
    fn upgrade(
        self,
        socket: C,
        id: Self::UpgradeIdentifier,
        ty: Endpoint,
    ) -> Self::Future {
        if self.enabled {
            future::Either::B(self.inner.upgrade(socket, id, ty))
        } else {
            future::Either::A(future::empty())
        }
    }
}

/// Iterator that is toggleable.
#[derive(Debug, Clone)]
pub struct ToggleableIter<I> {
    inner: I,
    // It is expected that `enabled` doesn't change once the iterator has been created.
    enabled: bool,
}

impl<I> Iterator for ToggleableIter<I>
where I: Iterator
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.enabled {
            self.inner.next()
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.enabled {
            self.inner.size_hint()
        } else {
            (0, Some(0))
        }
    }
}

impl<I> ExactSizeIterator for ToggleableIter<I>
where I: ExactSizeIterator {}
