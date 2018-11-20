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

use bytes::Bytes;
use futures::future::Either;
use crate::{
    either::{EitherOutput, EitherError, EitherFuture2},
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo}
};

/// A type to represent two possible upgrade types (inbound or outbound).
#[derive(Debug, Clone)]
pub enum EitherUpgrade<A, B> { A(A), B(B) }

impl<A, B> UpgradeInfo for EitherUpgrade<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo
{
    type UpgradeId = Either<A::UpgradeId, B::UpgradeId>;
    type NamesIter = EitherIter<A::NamesIter, B::NamesIter>;

    fn protocol_names(&self) -> Self::NamesIter {
        match self {
            EitherUpgrade::A(a) => EitherIter::A(a.protocol_names()),
            EitherUpgrade::B(b) => EitherIter::B(b.protocol_names())
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

    fn upgrade_inbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        match (self, id) {
            (EitherUpgrade::A(a), Either::A(id)) => EitherFuture2::A(a.upgrade_inbound(sock, id)),
            (EitherUpgrade::B(b), Either::B(id)) => EitherFuture2::B(b.upgrade_inbound(sock, id)),
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_inbound")
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

    fn upgrade_outbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        match (self, id) {
            (EitherUpgrade::A(a), Either::A(id)) => EitherFuture2::A(a.upgrade_outbound(sock, id)),
            (EitherUpgrade::B(b), Either::B(id)) => EitherFuture2::B(b.upgrade_outbound(sock, id)),
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_outbound")
        }
    }
}

/// A type to represent two possible `Iterator` types.
#[derive(Debug, Clone)]
pub enum EitherIter<A, B> { A(A), B(B) }

impl<A, B, AId, BId> Iterator for EitherIter<A, B>
where
    A: Iterator<Item = (Bytes, AId)>,
    B: Iterator<Item = (Bytes, BId)>,
{
    type Item = (Bytes, Either<AId, BId>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            EitherIter::A(a) => a.next().map(|(name, id)| (name, Either::A(id))),
            EitherIter::B(b) => b.next().map(|(name, id)| (name, Either::B(id)))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            EitherIter::A(a) => a.size_hint(),
            EitherIter::B(b) => b.size_hint()
        }
    }
}

