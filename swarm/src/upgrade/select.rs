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

use crate::upgrade::either::EitherFuture;
use crate::upgrade::{InboundUpgrade, IntoIteratorSend, OutboundUpgrade, UpgradeInfo};
use crate::Stream;
use either::Either;
use futures::future;
use std::iter::{Chain, Map};

/// Upgrade that combines two upgrades into one. Supports all the protocols supported by either
/// sub-upgrade.
///
/// The protocols supported by the first element have a higher priority.
#[derive(Debug, Clone)]
pub struct SelectUpgrade<A, B>(A, B);

impl<A, B> SelectUpgrade<A, B> {
    /// Combines two upgrades into an `SelectUpgrade`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(a: A, b: B) -> Self {
        SelectUpgrade(a, b)
    }
}

impl<A, B> UpgradeInfo for SelectUpgrade<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo,
{
    type Info = Either<A::Info, B::Info>;
    type InfoIter = Chain<
        Map<<A::InfoIter as IntoIteratorSend>::IntoIter, fn(A::Info) -> Self::Info>,
        Map<<B::InfoIter as IntoIteratorSend>::IntoIter, fn(B::Info) -> Self::Info>,
    >;

    fn protocol_info(&self) -> Self::InfoIter {
        let a = self
            .0
            .protocol_info()
            .into_iter_send()
            .map(Either::Left as fn(A::Info) -> _);
        let b = self
            .1
            .protocol_info()
            .into_iter_send()
            .map(Either::Right as fn(B::Info) -> _);

        a.chain(b)
    }
}

impl<A, B, TA, TB, EA, EB> InboundUpgrade for SelectUpgrade<A, B>
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
        match info {
            Either::Left(info) => EitherFuture::First(self.0.upgrade_inbound(sock, info)),
            Either::Right(info) => EitherFuture::Second(self.1.upgrade_inbound(sock, info)),
        }
    }
}

impl<A, B, TA, TB, EA, EB> OutboundUpgrade for SelectUpgrade<A, B>
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
        match info {
            Either::Left(info) => EitherFuture::First(self.0.upgrade_outbound(sock, info)),
            Either::Right(info) => EitherFuture::Second(self.1.upgrade_outbound(sock, info)),
        }
    }
}
