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

use futures::prelude::*;
use crate::upgrade::Upgrade;

/// Wraps around an upgrade and applies a closure to the output.
#[derive(Debug, Clone)]
pub struct Map<U, F> { upgrade: U, fun: F }

/// Wraps around an upgrade and applies a closure to the output.
pub fn map<U, F>(upgrade: U, fun: F) -> Map<U, F> {
    Map { upgrade, fun }
}

impl<T, U, F, A> Upgrade<T> for Map<U, F>
where
    U: Upgrade<T>,
    F: FnOnce(U::Output) -> A
{
    type UpgradeId = U::UpgradeId;
    type NamesIter = U::NamesIter;
    type Output = A;
    type Error = U::Error;
    type Future = MapFuture<U::Future, F>;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }

    fn upgrade(self, input: T, id: Self::UpgradeId) -> Self::Future {
        MapFuture {
            inner: self.upgrade.upgrade(input, id),
            fun: Some(self.fun)
        }
    }
}

/// Wraps around an upgrade and applies a closure to the error.
#[derive(Debug, Clone)]
pub struct MapErr<U, F> { upgrade: U, fun: F }

/// Wraps around an upgrade and applies a closure to the error.
pub fn map_err<U, F>(upgrade: U, fun: F) -> MapErr<U, F> {
    MapErr { upgrade, fun }
}

impl<T, U, F, E> Upgrade<T> for MapErr<U, F>
where
    U: Upgrade<T>,
    F: FnOnce(U::Error) -> E
{
    type UpgradeId = U::UpgradeId;
    type NamesIter = U::NamesIter;
    type Output = U::Output;
    type Error = E;
    type Future = MapErrFuture<U::Future, F>;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }

    fn upgrade(self, input: T, id: Self::UpgradeId) -> Self::Future {
        MapErrFuture {
            inner: self.upgrade.upgrade(input, id),
            fun: Some(self.fun)
        }
    }
}

#[derive(Debug)]
pub struct MapFuture<T, F> { inner: T, fun: Option<F> }

impl<T, A, F, B> Future for MapFuture<T, F>
where
    T: Future<Item = A>,
    F: FnOnce(A) -> B
{
    type Item = B;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.inner.poll());
        let fun = self.fun.take().expect("MapFuture has not resolved yet.");
        Ok(Async::Ready(fun(item)))
    }
}

#[derive(Debug)]
pub struct MapErrFuture<T, F> { inner: T, fun: Option<F> }

impl<T, E, F, A> Future for MapErrFuture<T, F>
where
    T: Future<Error = E>,
    F: FnOnce(E) -> A,
{
    type Item = T::Item;
    type Error = A;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(x)) => Ok(Async::Ready(x)),
            Err(e) => {
                let f = self.fun.take().expect("MapErrFuture has not resolved yet.");
                Err(f(e))
            }
        }
    }
}

