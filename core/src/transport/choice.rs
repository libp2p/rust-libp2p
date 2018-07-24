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

use either::{EitherListenStream, EitherListenUpgrade, EitherOutput};
use futures::{prelude::*, future};
use multiaddr::Multiaddr;
use std::io::Error as IoError;
use transport::{MuxedTransport, Transport};

/// Struct returned by `or_transport()`.
#[derive(Debug, Copy, Clone)]
pub struct OrTransport<A, B>(A, B);

impl<A, B> OrTransport<A, B> {
    pub fn new(a: A, b: B) -> OrTransport<A, B> {
        OrTransport(a, b)
    }
}

impl<A, B> Transport for OrTransport<A, B>
where
    A: Transport,
    B: Transport,
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Listener = EitherListenStream<A::Listener, B::Listener>;
    type ListenerUpgrade = EitherListenUpgrade<A::ListenerUpgrade, B::ListenerUpgrade>;
    type MultiaddrFuture = future::Either<A::MultiaddrFuture, B::MultiaddrFuture>;
    type Dial =
        EitherListenUpgrade<<A::Dial as IntoFuture>::Future, <B::Dial as IntoFuture>::Future>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let (first, addr) = match self.0.listen_on(addr) {
            Ok((connec, addr)) => return Ok((EitherListenStream::First(connec), addr)),
            Err(err) => err,
        };

        match self.1.listen_on(addr) {
            Ok((connec, addr)) => Ok((EitherListenStream::Second(connec), addr)),
            Err((second, addr)) => Err((OrTransport(first, second), addr)),
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let (first, addr) = match self.0.dial(addr) {
            Ok(connec) => return Ok(EitherListenUpgrade::First(connec)),
            Err(err) => err,
        };

        match self.1.dial(addr) {
            Ok(connec) => Ok(EitherListenUpgrade::Second(connec)),
            Err((second, addr)) => Err((OrTransport(first, second), addr)),
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        let first = self.0.nat_traversal(server, observed);
        if let Some(first) = first {
            return Some(first);
        }

        self.1.nat_traversal(server, observed)
    }
}

impl<A, B> MuxedTransport for OrTransport<A, B>
where
    A: MuxedTransport,
    B: MuxedTransport,
    A::Incoming: 'static,        // TODO: meh :-/
    B::Incoming: 'static,        // TODO: meh :-/
    A::IncomingUpgrade: 'static, // TODO: meh :-/
    B::IncomingUpgrade: 'static, // TODO: meh :-/
    A::Output: 'static,          // TODO: meh :-/
    B::Output: 'static,          // TODO: meh :-/
{
    type Incoming = Box<Future<Item = Self::IncomingUpgrade, Error = IoError>>;
    type IncomingUpgrade =
        Box<Future<Item = (EitherOutput<A::Output, B::Output>, Self::MultiaddrFuture), Error = IoError>>;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        let first = self.0.next_incoming().map(|out| {
            let fut = out.map(move |(v, addr)| (EitherOutput::First(v), future::Either::A(addr)));
            Box::new(fut) as Box<Future<Item = _, Error = _>>
        });
        let second = self.1.next_incoming().map(|out| {
            let fut = out.map(move |(v, addr)| (EitherOutput::Second(v), future::Either::B(addr)));
            Box::new(fut) as Box<Future<Item = _, Error = _>>
        });
        let future = first.select(second).map(|(i, _)| i).map_err(|(e, _)| e);
        Box::new(future) as Box<_>
    }
}
