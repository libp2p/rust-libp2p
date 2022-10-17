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
    either::{EitherError, EitherFuture2, EitherName, EitherOutput},
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
};

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
    type Info = EitherName<A::Info, B::Info>;
    type InfoIter = EitherIter<
        <A::InfoIter as IntoIterator>::IntoIter,
        <B::InfoIter as IntoIterator>::IntoIter,
    >;

    fn protocol_info(&self) -> Self::InfoIter {
        match self {
            EitherUpgrade::A(a) => EitherIter::A(a.protocol_info().into_iter()),
            EitherUpgrade::B(b) => EitherIter::B(b.protocol_info().into_iter()),
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

    fn upgrade_inbound(self, sock: C, info: Self::Info) -> Self::Future {
        match (self, info) {
            (EitherUpgrade::A(a), EitherName::A(info)) => {
                EitherFuture2::A(a.upgrade_inbound(sock, info))
            }
            (EitherUpgrade::B(b), EitherName::B(info)) => {
                EitherFuture2::B(b.upgrade_inbound(sock, info))
            }
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_inbound"),
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

    fn upgrade_outbound(self, sock: C, info: Self::Info) -> Self::Future {
        match (self, info) {
            (EitherUpgrade::A(a), EitherName::A(info)) => {
                EitherFuture2::A(a.upgrade_outbound(sock, info))
            }
            (EitherUpgrade::B(b), EitherName::B(info)) => {
                EitherFuture2::B(b.upgrade_outbound(sock, info))
            }
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_outbound"),
        }
    }
}

/// A type to represent two possible `Iterator` types.
#[derive(Debug, Clone)]
pub enum EitherIter<A, B> {
    A(A),
    B(B),
}

impl<A, B> Iterator for EitherIter<A, B>
where
    A: Iterator,
    B: Iterator,
{
    type Item = EitherName<A::Item, B::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            EitherIter::A(a) => a.next().map(EitherName::A),
            EitherIter::B(b) => b.next().map(EitherName::B),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            EitherIter::A(a) => a.size_hint(),
            EitherIter::B(b) => b.size_hint(),
        }
    }
}
