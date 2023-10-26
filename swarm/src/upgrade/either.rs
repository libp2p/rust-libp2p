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

use crate::upgrade::{InboundUpgrade, IntoIteratorSend, OutboundUpgrade, UpgradeInfo};
use crate::{Stream, StreamProtocol};
use either::Either;
use futures::{future, TryFuture};
use std::future::Future;
use std::iter::Map;
use std::pin::Pin;
use std::task::{Context, Poll};

impl<A, B> UpgradeInfo for Either<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo,
{
    type Info = Either<A::Info, B::Info>;
    type InfoIter = Either<
        Map<<A::InfoIter as IntoIteratorSend>::IntoIter, fn(A::Info) -> Self::Info>,
        Map<<B::InfoIter as IntoIteratorSend>::IntoIter, fn(B::Info) -> Self::Info>,
    >;

    fn protocol_info(&self) -> Self::InfoIter {
        match self {
            Either::Left(a) => Either::Left(a.protocol_info().into_iter_send().map(Either::Left)),
            Either::Right(b) => {
                Either::Right(b.protocol_info().into_iter_send().map(Either::Right))
            }
        }
    }
}

impl<A, B> From<Either<A, B>> for StreamProtocol
where
    StreamProtocol: From<A>,
    StreamProtocol: From<B>,
{
    fn from(value: Either<A, B>) -> Self {
        match value {
            Either::Left(a) => StreamProtocol::from(a),
            Either::Right(b) => StreamProtocol::from(b),
        }
    }
}

impl<A, B, TA, TB, EA, EB> InboundUpgrade for Either<A, B>
where
    A: InboundUpgrade<Output = TA, Error = EA>,
    B: InboundUpgrade<Output = TB, Error = EB>,
    TA: Send + 'static,
    TB: Send + 'static,
    EA: Send + 'static,
    EB: Send + 'static,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_inbound(self, sock: Stream, info: Self::Info) -> Self::Future {
        match (self, info) {
            (Either::Left(a), Either::Left(info)) => {
                EitherFuture::First(a.upgrade_inbound(sock, info))
            }
            (Either::Right(b), Either::Right(info)) => {
                EitherFuture::Second(b.upgrade_inbound(sock, info))
            }
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_inbound"),
        }
    }
}

impl<A, B, TA, TB, EA, EB> OutboundUpgrade for Either<A, B>
where
    A: OutboundUpgrade<Output = TA, Error = EA>,
    B: OutboundUpgrade<Output = TB, Error = EB>,
    TA: Send + 'static,
    TB: Send + 'static,
    EA: Send + 'static,
    EB: Send + 'static,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_outbound(self, sock: Stream, info: Self::Info) -> Self::Future {
        match (self, info) {
            (Either::Left(a), Either::Left(info)) => {
                EitherFuture::First(a.upgrade_outbound(sock, info))
            }
            (Either::Right(b), Either::Right(info)) => {
                EitherFuture::Second(b.upgrade_outbound(sock, info))
            }
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_outbound"),
        }
    }
}

/// Implements `Future` and dispatches all method calls to either `First` or `Second`.
#[pin_project::pin_project(project = EitherFutureProj)]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture<A, B> {
    First(#[pin] A),
    Second(#[pin] B),
}

impl<AFuture, BFuture, AInner, BInner> Future for EitherFuture<AFuture, BFuture>
where
    AFuture: TryFuture<Ok = AInner>,
    BFuture: TryFuture<Ok = BInner>,
{
    type Output = Result<future::Either<AInner, BInner>, Either<AFuture::Error, BFuture::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherFutureProj::First(a) => TryFuture::try_poll(a, cx)
                .map_ok(future::Either::Left)
                .map_err(Either::Left),
            EitherFutureProj::Second(a) => TryFuture::try_poll(a, cx)
                .map_ok(future::Either::Right)
                .map_err(Either::Right),
        }
    }
}
