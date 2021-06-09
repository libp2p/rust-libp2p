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
    either::{EitherOutput, EitherError, EitherFuture2, EitherName},
    upgrade::{Role, Upgrade},
};

/// A type to represent two possible upgrade types (inbound or outbound).
#[derive(Debug, Clone)]
pub enum EitherUpgrade<A, B> { A(A), B(B) }

impl<A, B, C> Upgrade<C> for EitherUpgrade<A, B>
where
    A: Upgrade<C>,
    <A::InfoIter as IntoIterator>::IntoIter: Send,
    B: Upgrade<C>,
    <B::InfoIter as IntoIterator>::IntoIter: Send,
    C: Send + 'static,
{
    type Info = EitherName<A::Info, B::Info>;
    type InfoIter = EitherIter<
        <A::InfoIter as IntoIterator>::IntoIter,
        <B::InfoIter as IntoIterator>::IntoIter
    >;
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = EitherError<A::Error, B::Error>;
    type Future = EitherFuture2<A::Future, B::Future>;

    fn protocol_info(&self) -> Self::InfoIter {
        match self {
            EitherUpgrade::A(a) => EitherIter::A(a.protocol_info().into_iter()),
            EitherUpgrade::B(b) => EitherIter::B(b.protocol_info().into_iter())
        }
    }

    fn upgrade(self, sock: C, info: Self::Info, role: Role) -> Self::Future {
        match (self, info) {
            (EitherUpgrade::A(a), EitherName::A(info)) => EitherFuture2::A(a.upgrade(sock, info, role)),
            (EitherUpgrade::B(b), EitherName::B(info)) => EitherFuture2::B(b.upgrade(sock, info, role)),
            _ => panic!("Invalid invocation of EitherUpgrade::upgrade_inbound")
        }
    }
}

/// A type to represent two possible `Iterator` types.
#[derive(Debug, Clone)]
pub enum EitherIter<A, B> { A(A), B(B) }

impl<A, B> Iterator for EitherIter<A, B>
where
    A: Iterator,
    B: Iterator
{
    type Item = EitherName<A::Item, B::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            EitherIter::A(a) => a.next().map(EitherName::A),
            EitherIter::B(b) => b.next().map(EitherName::B)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            EitherIter::A(a) => a.size_hint(),
            EitherIter::B(b) => b.size_hint()
        }
    }
}
