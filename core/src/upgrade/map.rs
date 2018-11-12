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

use futures::prelude::*;
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

/// Applies a closure on the output of a connection upgrade.
#[inline]
pub fn map<U, F>(upgrade: U, map: F) -> Map<U, F> {
    Map { upgrade, map }
}

/// Application of a closure on the output of a connection upgrade.
#[derive(Debug, Copy, Clone)]
pub struct Map<U, F> {
    upgrade: U,
    map: F,
}

impl<C, U, F, O> ConnectionUpgrade<C> for Map<U, F>
where
    U: ConnectionUpgrade<C>,
    C: AsyncRead + AsyncWrite,
    F: FnOnce(U::Output) -> O,
{
    type NamesIter = U::NamesIter;
    type UpgradeIdentifier = U::UpgradeIdentifier;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }

    type Output = O;
    type Future = MapFuture<U::Future, F>;

    fn upgrade(
        self,
        socket: C,
        id: Self::UpgradeIdentifier,
        ty: Endpoint,
    ) -> Self::Future {
        MapFuture {
            inner: self.upgrade.upgrade(socket, id, ty),
            map: Some(self.map),
        }
    }
}

pub struct MapFuture<TInnerFut, TMap> {
    inner: TInnerFut,
    map: Option<TMap>,
}

impl<TInnerFut, TIn, TMap, TOut> Future for MapFuture<TInnerFut, TMap>
where TInnerFut: Future<Item = TIn>,
      TMap: FnOnce(TIn) -> TOut,
{
    type Item = TOut;
    type Error = TInnerFut::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.inner.poll());
        let map = self.map.take().expect("Future has already finished");
        Ok(Async::Ready(map(item)))
    }
}
