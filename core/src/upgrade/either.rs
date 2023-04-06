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

use crate::upgrade::UpgradeProtocols;
use crate::{
    either::EitherFuture,
    upgrade::{InboundUpgrade, OutboundUpgrade},
};
use either::Either;
use futures::future;
use multistream_select::Protocol;

impl<A, B> UpgradeProtocols for Either<A, B>
where
    A: UpgradeProtocols,
    B: UpgradeProtocols,
{
    fn protocols(&self) -> Vec<Protocol> {
        match self {
            Either::Left(a) => a.protocols(),
            Either::Right(b) => b.protocols(),
        }
    }
}

impl<C, A, B, TA, TB, EA, EB> InboundUpgrade<C> for Either<A, B>
where
    A: InboundUpgrade<C, Output = TA, Error = EA>,
    B: InboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_inbound(self, sock: C, selected_protocol: Protocol) -> Self::Future {
        match self {
            Either::Left(a) => EitherFuture::First(a.upgrade_inbound(sock, selected_protocol)),
            Either::Right(b) => EitherFuture::Second(b.upgrade_inbound(sock, selected_protocol)),
        }
    }
}

impl<C, A, B, TA, TB, EA, EB> OutboundUpgrade<C> for Either<A, B>
where
    A: OutboundUpgrade<C, Output = TA, Error = EA>,
    B: OutboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_outbound(self, sock: C, selected_protocol: Protocol) -> Self::Future {
        match self {
            Either::Left(a) => EitherFuture::First(a.upgrade_outbound(sock, selected_protocol)),
            Either::Right(b) => EitherFuture::Second(b.upgrade_outbound(sock, selected_protocol)),
        }
    }
}
