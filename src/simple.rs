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

use bytes::Bytes;
use core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use futures::prelude::*;
use std::{iter, io::Error as IoError, sync::Arc};
use tokio_io::{AsyncRead, AsyncWrite};

/// Implementation of `ConnectionUpgrade`. Convenient to use with small protocols.
#[derive(Debug)]
pub struct SimpleProtocol<F> {
    name: Bytes,
    // Note: we put the closure `F` in an `Arc` because Rust closures aren't automatically clonable
    // yet.
    upgrade: Arc<F>,
}

impl<F> SimpleProtocol<F> {
    /// Builds a `SimpleProtocol`.
    #[inline]
    pub fn new<N>(name: N, upgrade: F) -> SimpleProtocol<F>
    where
        N: Into<Bytes>,
    {
        SimpleProtocol {
            name: name.into(),
            upgrade: Arc::new(upgrade),
        }
    }
}

impl<F> Clone for SimpleProtocol<F> {
    #[inline]
    fn clone(&self) -> Self {
        SimpleProtocol {
            name: self.name.clone(),
            upgrade: self.upgrade.clone(),
        }
    }
}

impl<F> UpgradeInfo for SimpleProtocol<F> {
    type UpgradeId = ();
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((self.name.clone(), ()))
    }
}

impl<C, F, O> InboundUpgrade<C> for SimpleProtocol<F>
where
    C: AsyncRead + AsyncWrite,
    F: Fn(C) -> O,
    O: IntoFuture<Error = IoError>,
    O::Future: Send + 'static,
{
    type Output = O::Item;
    type Error = IoError;
    type Future = Box<Future<Item = O::Item, Error = Self::Error> + Send>;

    #[inline]
    fn upgrade_inbound(self, socket: C, _: Self::UpgradeId) -> Self::Future {
        let upgrade = &self.upgrade;
        let fut = upgrade(socket).into_future().from_err();
        Box::new(fut) as Box<_>
    }
}

impl<C, F, O> OutboundUpgrade<C> for SimpleProtocol<F>
where
    C: AsyncRead + AsyncWrite,
    F: Fn(C) -> O,
    O: IntoFuture<Error = IoError>,
    O::Future: Send + 'static,
{
    type Output = O::Item;
    type Error = IoError;
    type Future = Box<Future<Item = O::Item, Error = Self::Error> + Send>;

    #[inline]
    fn upgrade_outbound(self, socket: C, _: Self::UpgradeId) -> Self::Future {
        let upgrade = &self.upgrade;
        let fut = upgrade(socket).into_future().from_err();
        Box::new(fut) as Box<_>
    }
}
