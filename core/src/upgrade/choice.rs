// Copyright 2017 Parity Technologies (UK) Ltd.
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
use futures::{future, prelude::*};
use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

/// Builds a new `ConnectionUpgrade` that chooses between `A` and `B`.
///
/// If both `A` and `B` are supported by the remote, then `A` will be chosen.
// TODO: write a test for this ^
#[inline]
pub fn or<A, B>(me: A, other: B) -> OrUpgrade<A, B> {
    OrUpgrade(me, other)
}

/// See `upgrade::or`.
#[derive(Debug, Copy, Clone)]
pub struct OrUpgrade<A, B>(A, B);

impl<C, A, B, O, Maf> ConnectionUpgrade<C, Maf> for OrUpgrade<A, B>
where
    C: AsyncRead + AsyncWrite,
    A: ConnectionUpgrade<C, Maf, Output = O>,
    B: ConnectionUpgrade<C, Maf, Output = O>,
{
    type NamesIter = NamesIterChain<A::NamesIter, B::NamesIter>;
    type UpgradeIdentifier = EitherUpgradeIdentifier<A::UpgradeIdentifier, B::UpgradeIdentifier>;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        NamesIterChain {
            first: self.0.protocol_names(),
            second: self.1.protocol_names(),
        }
    }

    type Output = O;
    type MultiaddrFuture = future::Either<A::MultiaddrFuture, B::MultiaddrFuture>;
    type Future = EitherConnUpgrFuture<A::Future, B::Future>;

    #[inline]
    fn upgrade(
        self,
        socket: C,
        id: Self::UpgradeIdentifier,
        ty: Endpoint,
        remote_addr: Maf,
    ) -> Self::Future {
        match id {
            EitherUpgradeIdentifier::First(id) => {
                EitherConnUpgrFuture::First(self.0.upgrade(socket, id, ty, remote_addr))
            }
            EitherUpgradeIdentifier::Second(id) => {
                EitherConnUpgrFuture::Second(self.1.upgrade(socket, id, ty, remote_addr))
            }
        }
    }
}

/// Internal struct used by the `OrUpgrade` trait.
#[derive(Debug, Copy, Clone)]
pub enum EitherUpgradeIdentifier<A, B> {
    First(A),
    Second(B),
}

/// Implements `Future` and redirects calls to either `First` or `Second`.
///
/// Additionally, the output will be wrapped inside a `EitherOutput`.
///
// TODO: This type is needed because of the lack of `impl Trait` in stable Rust.
//         If Rust had impl Trait we could use the Either enum from the futures crate and add some
//         modifiers to it. This custom enum is a combination of Either and these modifiers.
#[derive(Debug, Copy, Clone)]
pub enum EitherConnUpgrFuture<A, B> {
    First(A),
    Second(B),
}

impl<A, B, O, Ma, Mb> Future for EitherConnUpgrFuture<A, B>
where
    A: Future<Error = IoError, Item = (O, Ma)>,
    B: Future<Error = IoError, Item = (O, Mb)>,
{
    type Item = (O, future::Either<Ma, Mb>);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self {
            &mut EitherConnUpgrFuture::First(ref mut a) => {
                let (item, fut) = try_ready!(a.poll());
                Ok(Async::Ready((item, future::Either::A(fut))))
            }
            &mut EitherConnUpgrFuture::Second(ref mut b) => {
                let (item, fut) = try_ready!(b.poll());
                Ok(Async::Ready((item, future::Either::B(fut))))
            }
        }
    }
}

/// Internal type used by the `OrUpgrade` struct.
///
/// > **Note**: This type is needed because of the lack of `-> impl Trait` in Rust. It can be
/// >           removed eventually.
#[derive(Debug, Copy, Clone)]
pub struct NamesIterChain<A, B> {
    first: A,
    second: B,
}

impl<A, B, AId, BId> Iterator for NamesIterChain<A, B>
where
    A: Iterator<Item = (Bytes, AId)>,
    B: Iterator<Item = (Bytes, BId)>,
{
    type Item = (Bytes, EitherUpgradeIdentifier<AId, BId>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some((name, id)) = self.first.next() {
            return Some((name, EitherUpgradeIdentifier::First(id)));
        }
        if let Some((name, id)) = self.second.next() {
            return Some((name, EitherUpgradeIdentifier::Second(id)));
        }
        None
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (min1, max1) = self.first.size_hint();
        let (min2, max2) = self.second.size_hint();
        let max = match (max1, max2) {
            (Some(max1), Some(max2)) => max1.checked_add(max2),
            _ => None,
        };
        (min1.saturating_add(min2), max)
    }
}
