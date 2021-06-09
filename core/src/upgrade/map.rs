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
use futures::prelude::*;
use std::{pin::Pin, task::Context, task::Poll};

/// Wraps around an upgrade and applies a closure to the output.
#[derive(Debug, Clone)]
pub struct MapUpgrade<U, F> { upgrade: U, fun: F }

impl<U, F> MapUpgrade<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapUpgrade { upgrade, fun }
    }
}

impl<U, C, F, T> Upgrade<C> for MapUpgrade<U, F>
where
    U: Upgrade<C>,
    F: FnOnce(U::Output) -> T + Send + 'static,
    T: Send + 'static,
{
    type Info = U::Info;
    type InfoIter = U::InfoIter;
    type Output = T;
    type Error = U::Error;
    type Future = MapFuture<U::Future, F>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrade.protocol_info()
    }

    fn upgrade(self, sock: C, info: Self::Info, role: Role) -> Self::Future {
        MapFuture {
            inner: self.upgrade.upgrade(sock, info, role),
            map: Some(self.fun)
        }
    }
}

/// Wraps around an upgrade and applies a closure to the error.
#[derive(Debug, Clone)]
pub struct MapUpgradeErr<U, F> { upgrade: U, fun: F }

impl<U, F> MapUpgradeErr<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapUpgradeErr { upgrade, fun }
    }
}

impl<U, C, F, T> Upgrade<C> for MapUpgradeErr<U, F>
where
    U: Upgrade<C>,
    F: FnOnce(U::Error) -> T + Send + 'static,
    T: Send + 'static,
{
    type Info = U::Info;
    type InfoIter = U::InfoIter;
    type Output = U::Output;
    type Error = T;
    type Future = MapErrFuture<U::Future, F>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrade.protocol_info()
    }

    fn upgrade(self, sock: C, info: Self::Info, role: Role) -> Self::Future {
        MapErrFuture {
            fut: self.upgrade.upgrade(sock, info, role),
            fun: Some(self.fun)
        }
    }
}

#[pin_project::pin_project]
pub struct MapFuture<TInnerFut, TMap> {
    #[pin]
    inner: TInnerFut,
    map: Option<TMap>,
}

impl<TInnerFut, TIn, TMap, TOut> Future for MapFuture<TInnerFut, TMap>
where
    TInnerFut: TryFuture<Ok = TIn>,
    TMap: FnOnce(TIn) -> TOut,
{
    type Output = Result<TOut, TInnerFut::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let item = match TryFuture::try_poll(this.inner, cx) {
            Poll::Ready(Ok(v)) => v,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => return Poll::Pending,
        };

        let map = this.map.take().expect("Future has already finished");
        Poll::Ready(Ok(map(item)))
    }
}

#[pin_project::pin_project]
pub struct MapErrFuture<T, F> {
    #[pin]
    fut: T,
    fun: Option<F>,
}

impl<T, E, F, A> Future for MapErrFuture<T, F>
where
    T: TryFuture<Error = E>,
    F: FnOnce(E) -> A,
{
    type Output = Result<T::Ok, A>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match TryFuture::try_poll(this.fut, cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(x)) => Poll::Ready(Ok(x)),
            Poll::Ready(Err(e)) => {
                let f = this.fun.take().expect("Future has not resolved yet");
                Poll::Ready(Err(f(e)))
            }
        }
    }
}
