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
use crate::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};

/// Upgrade that combines two upgrades into one. Supports all the protocols supported by either
/// sub-upgrade.
///
/// The protocols supported by the first element have a higher priority.
#[derive(Debug, Clone)]
pub struct OrUpgrade<A, B>(A, B);

impl<A, B> OrUpgrade<A, B> {
    /// Combines two upgrades into an `OrUpgrade`.
    ///
    /// The protocols supported by the first element have a higher priority.
    pub fn new(a: A, b: B) -> Self {
        OrUpgrade(a, b)
    }
}

impl<A, B> UpgradeInfo for OrUpgrade<A, B>
where
    A: UpgradeInfo,
    B: UpgradeInfo
{
    type UpgradeId = Either<A::UpgradeId, B::UpgradeId>;
    type NamesIter = NamesIterChain<A::NamesIter, B::NamesIter>;

    fn protocol_names(&self) -> Self::NamesIter {
        NamesIterChain(self.0.protocol_names(), self.1.protocol_names())
    }
}

impl<C, A, B, T, E> InboundUpgrade<C> for OrUpgrade<A, B>
where
    A: InboundUpgrade<C, Output = T, Error = E>,
    B: InboundUpgrade<C, Output = T, Error = E>,
{
    type Output = T; // TODO: different output types
    type Error = E; // TODO: different error types
    type Future = Either<A::Future, B::Future>;

    fn upgrade_inbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        match id {
            Either::A(id) => Either::A(self.0.upgrade_inbound(sock, id)),
            Either::B(id) => Either::B(self.1.upgrade_inbound(sock, id))
        }
    }
}

impl<C, A, B, T, E> OutboundUpgrade<C> for OrUpgrade<A, B>
where
    A: OutboundUpgrade<C, Output = T, Error = E>,
    B: OutboundUpgrade<C, Output = T, Error = E>,
{
    type Output = T; // TODO: different output types
    type Error = E; // TODO: different error types
    type Future = Either<A::Future, B::Future>;

    fn upgrade_outbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        match id {
            Either::A(id) => Either::A(self.0.upgrade_outbound(sock, id)),
            Either::B(id) => Either::B(self.1.upgrade_outbound(sock, id))
        }
    }
}

/// Iterator that combines the protocol names of twp upgrades.
#[derive(Debug, Clone)]
pub struct NamesIterChain<A, B>(A, B);

impl<A, B, AId, BId> Iterator for NamesIterChain<A, B>
where
    A: Iterator<Item = (Bytes, AId)>,
    B: Iterator<Item = (Bytes, BId)>,
{
    type Item = (Bytes, Either<AId, BId>);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((name, id)) = self.0.next() {
            return Some((name, Either::A(id)))
        }
        if let Some((name, id)) = self.1.next() {
            return Some((name, Either::B(id)))
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

