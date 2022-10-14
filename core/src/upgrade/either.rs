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

use crate::{
    either::{EitherError, EitherFuture2, EitherOutput},
    upgrade::{InboundUpgrade, OutboundUpgrade, ProtocolName, UpgradeInfo},
};
use either::Either;

/// A type to represent two possible upgrade types (inbound or outbound).
#[derive(Debug, Clone)]
pub enum EitherUpgrade<A, B> {
    A(A),
    B(B),
}

impl<A, B> UpgradeInfo for EitherUpgrade<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo,
{
    type InfoIter =
        Either<<A::InfoIter as IntoIterator>::IntoIter, <B::InfoIter as IntoIterator>::IntoIter>;

    fn protocol_info(&self) -> Self::InfoIter {
        match self {
            EitherUpgrade::A(a) => Either::Left(a.protocol_info().into_iter()),
            EitherUpgrade::B(b) => Either::Right(b.protocol_info().into_iter()),
        }
    }
}

impl<C, A, B, TA, TB, EA, EB> InboundUpgrade<C> for EitherUpgrade<A, B>
where
    A: InboundUpgrade<C, Output = TA, Error = EA>,
    B: InboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = EitherOutput<TA, TB>;
    type Error = EitherError<EA, EB>;
    type Future = EitherFuture2<A::Future, B::Future>;

    fn upgrade_inbound(self, sock: C, info: ProtocolName) -> Self::Future {
        match self {
            EitherUpgrade::A(a) => EitherFuture2::A(a.upgrade_inbound(sock, info)),
            EitherUpgrade::B(b) => EitherFuture2::B(b.upgrade_inbound(sock, info)),
        }
    }
}

impl<C, A, B, TA, TB, EA, EB> OutboundUpgrade<C> for EitherUpgrade<A, B>
where
    A: OutboundUpgrade<C, Output = TA, Error = EA>,
    B: OutboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = EitherOutput<TA, TB>;
    type Error = EitherError<EA, EB>;
    type Future = EitherFuture2<A::Future, B::Future>;

    fn upgrade_outbound(self, sock: C, info: ProtocolName) -> Self::Future {
        match self {
            EitherUpgrade::A(a) => EitherFuture2::A(a.upgrade_outbound(sock, info)),
            EitherUpgrade::B(b) => EitherFuture2::B(b.upgrade_outbound(sock, info)),
        }
    }
}
