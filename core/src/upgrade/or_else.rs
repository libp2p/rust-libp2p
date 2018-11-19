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

use futures::{future::Either, prelude::*};
use crate::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};

/// Create a future from the `Error` of the inner upgrade.
#[derive(Debug, Clone)]
pub struct InboundUpgradeOrElse<U, F> { upgrade: U, fun: F }

impl<U, F> InboundUpgradeOrElse<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        InboundUpgradeOrElse { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for InboundUpgradeOrElse<U, F>
where
    U: UpgradeInfo
{
    type UpgradeId = U::UpgradeId;
    type NamesIter = U::NamesIter;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }
}

impl<C, U, F, T> InboundUpgrade<C> for InboundUpgradeOrElse<U, F>
where
    U: InboundUpgrade<C>,
    F: FnOnce(U::Error) -> T,
    T: IntoFuture<Item = U::Output>
{
    type Output = U::Output;
    type Error = <T::Future as Future>::Error;
    type Future = OrElseFuture<U::Future, F, T::Future>;

    fn upgrade_inbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        OrElseFuture {
            inner: Either::A(self.upgrade.upgrade_inbound(sock, id)),
            bind: Some(self.fun)
        }
    }
}

impl<C, U, F> OutboundUpgrade<C> for InboundUpgradeOrElse<U, F>
where
    U: OutboundUpgrade<C>
{
    type Output = U::Output;
    type Error = U::Error;
    type Future = U::Future;

    fn upgrade_outbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        self.upgrade.upgrade_outbound(sock, id)
    }
}

/// Create a future from the `Error` of the inner upgrade.
#[derive(Debug, Clone)]
pub struct OutboundUpgradeOrElse<U, F> { upgrade: U, fun: F }

impl<U, F> OutboundUpgradeOrElse<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        OutboundUpgradeOrElse { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for OutboundUpgradeOrElse<U, F>
where
    U: UpgradeInfo
{
    type UpgradeId = U::UpgradeId;
    type NamesIter = U::NamesIter;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }
}

impl<C, U, F, T> OutboundUpgrade<C> for OutboundUpgradeOrElse<U, F>
where
    U: OutboundUpgrade<C>,
    F: FnOnce(U::Error) -> T,
    T: IntoFuture<Item = U::Output>
{
    type Output = U::Output;
    type Error = <T::Future as Future>::Error;
    type Future = OrElseFuture<U::Future, F, T::Future>;

    fn upgrade_outbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        OrElseFuture {
            inner: Either::A(self.upgrade.upgrade_outbound(sock, id)),
            bind: Some(self.fun)
        }
    }
}

impl<C, U, F> InboundUpgrade<C> for OutboundUpgradeOrElse<U, F>
where
    U: InboundUpgrade<C>
{
    type Output = U::Output;
    type Error = U::Error;
    type Future = U::Future;

    fn upgrade_inbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        self.upgrade.upgrade_inbound(sock, id)
    }
}

pub struct OrElseFuture<T, F, U> {
    inner: Either<T, U>,
    bind: Option<F>
}

impl<T, A, F, B> Future for OrElseFuture<T, F, B::Future>
where
    T: Future<Error = A>,
    F: FnOnce(A) -> B,
    B: IntoFuture<Item = T::Item>
{
    type Item = T::Item;
    type Error = <B::Future as Future>::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let future = match self.inner {
            Either::A(ref mut future) => {
                match future.poll() {
                    Ok(a) => return Ok(a),
                    Err(e) => {
                        let bind = self.bind.take().expect("Future has already finished");
                        bind(e).into_future()
                    }
                }
            }
            Either::B(ref mut future) => return future.poll()
        };
        self.inner = Either::B(future);
        Ok(Async::NotReady)
    }
}

