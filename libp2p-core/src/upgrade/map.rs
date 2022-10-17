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

use crate::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use futures::prelude::*;
use std::{pin::Pin, task::Context, task::Poll};

/// Wraps around an upgrade and applies a closure to the output.
#[derive(Debug, Clone)]
pub struct MapInboundUpgrade<U, F> {
    upgrade: U,
    fun: F,
}

impl<U, F> MapInboundUpgrade<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapInboundUpgrade { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for MapInboundUpgrade<U, F>
where
    U: UpgradeInfo,
{
    type Info = U::Info;
    type InfoIter = U::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrade.protocol_info()
    }
}

impl<C, U, F, T> InboundUpgrade<C> for MapInboundUpgrade<U, F>
where
    U: InboundUpgrade<C>,
    F: FnOnce(U::Output) -> T,
{
    type Output = T;
    type Error = U::Error;
    type Future = MapFuture<U::Future, F>;

    fn upgrade_inbound(self, sock: C, info: Self::Info) -> Self::Future {
        MapFuture {
            inner: self.upgrade.upgrade_inbound(sock, info),
            map: Some(self.fun),
        }
    }
}

impl<C, U, F> OutboundUpgrade<C> for MapInboundUpgrade<U, F>
where
    U: OutboundUpgrade<C>,
{
    type Output = U::Output;
    type Error = U::Error;
    type Future = U::Future;

    fn upgrade_outbound(self, sock: C, info: Self::Info) -> Self::Future {
        self.upgrade.upgrade_outbound(sock, info)
    }
}

/// Wraps around an upgrade and applies a closure to the output.
#[derive(Debug, Clone)]
pub struct MapOutboundUpgrade<U, F> {
    upgrade: U,
    fun: F,
}

impl<U, F> MapOutboundUpgrade<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapOutboundUpgrade { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for MapOutboundUpgrade<U, F>
where
    U: UpgradeInfo,
{
    type Info = U::Info;
    type InfoIter = U::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrade.protocol_info()
    }
}

impl<C, U, F> InboundUpgrade<C> for MapOutboundUpgrade<U, F>
where
    U: InboundUpgrade<C>,
{
    type Output = U::Output;
    type Error = U::Error;
    type Future = U::Future;

    fn upgrade_inbound(self, sock: C, info: Self::Info) -> Self::Future {
        self.upgrade.upgrade_inbound(sock, info)
    }
}

impl<C, U, F, T> OutboundUpgrade<C> for MapOutboundUpgrade<U, F>
where
    U: OutboundUpgrade<C>,
    F: FnOnce(U::Output) -> T,
{
    type Output = T;
    type Error = U::Error;
    type Future = MapFuture<U::Future, F>;

    fn upgrade_outbound(self, sock: C, info: Self::Info) -> Self::Future {
        MapFuture {
            inner: self.upgrade.upgrade_outbound(sock, info),
            map: Some(self.fun),
        }
    }
}

/// Wraps around an upgrade and applies a closure to the error.
#[derive(Debug, Clone)]
pub struct MapInboundUpgradeErr<U, F> {
    upgrade: U,
    fun: F,
}

impl<U, F> MapInboundUpgradeErr<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapInboundUpgradeErr { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for MapInboundUpgradeErr<U, F>
where
    U: UpgradeInfo,
{
    type Info = U::Info;
    type InfoIter = U::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrade.protocol_info()
    }
}

impl<C, U, F, T> InboundUpgrade<C> for MapInboundUpgradeErr<U, F>
where
    U: InboundUpgrade<C>,
    F: FnOnce(U::Error) -> T,
{
    type Output = U::Output;
    type Error = T;
    type Future = MapErrFuture<U::Future, F>;

    fn upgrade_inbound(self, sock: C, info: Self::Info) -> Self::Future {
        MapErrFuture {
            fut: self.upgrade.upgrade_inbound(sock, info),
            fun: Some(self.fun),
        }
    }
}

impl<C, U, F> OutboundUpgrade<C> for MapInboundUpgradeErr<U, F>
where
    U: OutboundUpgrade<C>,
{
    type Output = U::Output;
    type Error = U::Error;
    type Future = U::Future;

    fn upgrade_outbound(self, sock: C, info: Self::Info) -> Self::Future {
        self.upgrade.upgrade_outbound(sock, info)
    }
}

/// Wraps around an upgrade and applies a closure to the error.
#[derive(Debug, Clone)]
pub struct MapOutboundUpgradeErr<U, F> {
    upgrade: U,
    fun: F,
}

impl<U, F> MapOutboundUpgradeErr<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapOutboundUpgradeErr { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for MapOutboundUpgradeErr<U, F>
where
    U: UpgradeInfo,
{
    type Info = U::Info;
    type InfoIter = U::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrade.protocol_info()
    }
}

impl<C, U, F, T> OutboundUpgrade<C> for MapOutboundUpgradeErr<U, F>
where
    U: OutboundUpgrade<C>,
    F: FnOnce(U::Error) -> T,
{
    type Output = U::Output;
    type Error = T;
    type Future = MapErrFuture<U::Future, F>;

    fn upgrade_outbound(self, sock: C, info: Self::Info) -> Self::Future {
        MapErrFuture {
            fut: self.upgrade.upgrade_outbound(sock, info),
            fun: Some(self.fun),
        }
    }
}

impl<C, U, F> InboundUpgrade<C> for MapOutboundUpgradeErr<U, F>
where
    U: InboundUpgrade<C>,
{
    type Output = U::Output;
    type Error = U::Error;
    type Future = U::Future;

    fn upgrade_inbound(self, sock: C, info: Self::Info) -> Self::Future {
        self.upgrade.upgrade_inbound(sock, info)
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
