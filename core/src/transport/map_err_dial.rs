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

use crate::transport::Transport;
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::io;

/// See `Transport::map_err_dial`.
#[derive(Debug, Copy, Clone)]
pub struct MapErrDial<T, F> { transport: T, fun: F }

impl<T, F> MapErrDial<T, F> {
    /// Internal function that builds a `MapErrDial`.
    #[inline]
    pub(crate) fn new(transport: T, fun: F) -> MapErrDial<T, F> {
        MapErrDial { transport, fun }
    }
}

impl<T, F> Transport for MapErrDial<T, F>
where
    T: Transport,
    F: FnOnce(io::Error, Multiaddr) -> io::Error
{
    type Output = T::Output;
    type Listener = T::Listener;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = MapErrFuture<T::Dial, F>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let fun = self.fun;
        self.transport.listen_on(addr)
            .map_err(move |(transport, addr)| (MapErrDial { transport, fun }, addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match self.transport.dial(addr.clone()) {
            Ok(future) => Ok(MapErrFuture {
                inner: future,
                args: Some((self.fun, addr))
            }),
            Err((transport, addr)) => Err((MapErrDial { transport, fun: self.fun }, addr))
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

/// Custom `Future` to avoid boxing.
///
/// Applies a function to the inner future's error type.
#[derive(Debug, Clone)]
pub struct MapErrFuture<T, F> {
    inner: T,
    args: Option<(F, Multiaddr)>,
}

impl<T, E, F, A> Future for MapErrFuture<T, F>
where
    T: Future<Error = E>,
    F: FnOnce(E, Multiaddr) -> A
{
    type Item = T::Item;
    type Error = A;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(x)) => Ok(Async::Ready(x)),
            Err(e) => {
                let (f, a) = self.args.take().expect("MapErrFuture has already finished.");
                Err(f(e, a))
            }
        }
    }
}

