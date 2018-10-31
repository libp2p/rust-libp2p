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
    Multiaddr,
    either::{EitherOutput, EitherError, EitherFuture2, EitherIncoming},
    transport::{Dialer, Listener}
};

#[derive(Debug, Copy, Clone)]
pub struct OrDialer<A, B>(A, B);

impl<A, B> OrDialer<A, B> {
    pub fn new(a: A, b: B) -> Self {
        OrDialer(a, b)
    }
}

impl<A, B> Dialer for OrDialer<A, B>
where
    A: Dialer,
    B: Dialer
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = EitherError<A::Error, B::Error>;
    type Outbound = EitherFuture2<A::Outbound, B::Outbound>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        match self.0.dial(addr) {
            Ok(future) => Ok(EitherFuture2::A(future)),
            Err((a, addr)) =>
                match self.1.dial(addr) {
                    Ok(future) => Ok(EitherFuture2::B(future)),
                    Err((b, addr)) => Err((OrDialer(a, b), addr))
                }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct OrListener<A, B>(A, B);

impl<A, B> OrListener<A, B> {
    pub fn new(a: A, b: B) -> Self {
        OrListener(a, b)
    }
}

impl<A, B> Listener for OrListener<A, B>
where
    A: Listener,
    B: Listener
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = EitherError<A::Error, B::Error>;
    type Inbound = EitherIncoming<A::Inbound, B::Inbound>;
    type Upgrade = EitherFuture2<A::Upgrade, B::Upgrade>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        match self.0.listen_on(addr) {
            Ok((inc, addr)) => Ok((EitherIncoming::A(inc), addr)),
            Err((a, addr)) =>
                match self.1.listen_on(addr) {
                    Ok((inc, addr)) => Ok((EitherIncoming::B(inc), addr)),
                    Err((b, addr)) => Err((OrListener(a, b), addr))
                }
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.0.nat_traversal(server, observed).or_else(|| self.1.nat_traversal(server, observed))
    }
}

