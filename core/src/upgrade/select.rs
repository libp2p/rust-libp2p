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

use crate::either::EitherFuture;
use crate::upgrade::UpgradeProtocols;
use crate::{
    either::EitherName,
    upgrade::{InboundUpgrade, OutboundUpgrade},
};
use either::Either;
use futures::future;
use multistream_select::Protocol;

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

impl<A, B> UpgradeProtocols for SelectUpgrade<A, B>
where
    A: UpgradeProtocols,
    B: UpgradeProtocols,
{
    type Iter = std::iter::Chain<A::Iter, B::Iter>;

    fn protocols(&self) -> Self::Iter {
        self.0.protocols().chain(self.1.protocols())
    }
}

impl<C, A, B, TA, TB, EA, EB> InboundUpgrade<C> for SelectUpgrade<A, B>
where
    A: InboundUpgrade<C, Output = TA, Error = EA>,
    B: InboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_inbound(self, sock: C, selected_protocol: Protocol) -> Self::Future {
        if self.0.protocols().any(|p| p == selected_protocol) {
            return EitherFuture::First(self.0.upgrade_inbound(sock, selected_protocol));
        }

        if self.1.protocols().any(|p| p == selected_protocol) {
            return EitherFuture::Second(self.1.upgrade_inbound(sock, selected_protocol));
        }

        panic!("selected protocol is not supported by either upgrade")
    }
}

impl<C, A, B, TA, TB, EA, EB> OutboundUpgrade<C> for SelectUpgrade<A, B>
where
    A: OutboundUpgrade<C, Output = TA, Error = EA>,
    B: OutboundUpgrade<C, Output = TB, Error = EB>,
{
    type Output = future::Either<TA, TB>;
    type Error = Either<EA, EB>;
    type Future = EitherFuture<A::Future, B::Future>;

    fn upgrade_outbound(self, sock: C, selected_protocol: Protocol) -> Self::Future {
        if self.0.protocols().any(|p| p == selected_protocol) {
            return EitherFuture::First(self.0.upgrade_outbound(sock, selected_protocol));
        }

        if self.1.protocols().any(|p| p == selected_protocol) {
            return EitherFuture::Second(self.1.upgrade_outbound(sock, selected_protocol));
        }

        panic!("selected protocol is not supported by either upgrade")
    }
}

/// Iterator that combines the protocol names of twp upgrades.
#[derive(Debug, Clone)]
pub struct InfoIterChain<A, B>(A, B);

impl<A, B> Iterator for InfoIterChain<A, B>
where
    A: Iterator,
    B: Iterator,
{
    type Item = EitherName<A::Item, B::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(info) = self.0.next() {
            return Some(EitherName::A(info));
        }
        if let Some(info) = self.1.next() {
            return Some(EitherName::B(info));
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (min1, max1) = self.0.size_hint();
        let (min2, max2) = self.1.size_hint();
        let max = max1.and_then(move |m1| max2.and_then(move |m2| m1.checked_add(m2)));
        (min1.saturating_add(min2), max)
    }
}
