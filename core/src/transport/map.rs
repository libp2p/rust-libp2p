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

use crate::{nodes::raw_swarm::ConnectedPoint, transport::Transport};
use futures::prelude::*;
use multiaddr::Multiaddr;

/// See `Transport::map`.
#[derive(Debug, Copy, Clone)]
pub struct Map<T, F> { transport: T, fun: F }

impl<T, F> Map<T, F> {
    #[inline]
    pub(crate) fn new(transport: T, fun: F) -> Self {
        Map { transport, fun }
    }
}

impl<T, F, D> Transport for Map<T, F>
where
    T: Transport,
    F: FnOnce(T::Output, ConnectedPoint) -> D + Clone
{
    type Output = D;
    type Listener = MapStream<T::Listener, F>;
    type ListenerUpgrade = MapFuture<T::ListenerUpgrade, F>;
    type Dial = MapFuture<T::Dial, F>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        match self.transport.listen_on(addr) {
            Ok((stream, listen_addr)) => {
                let stream = MapStream {
                    stream,
                    listen_addr: listen_addr.clone(),
                    fun: self.fun
                };
                Ok((stream, listen_addr))
            }
            Err((transport, addr)) => Err((Map { transport, fun: self.fun }, addr)),
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match self.transport.dial(addr.clone()) {
            Ok(future) => {
                let p = ConnectedPoint::Dialer { address: addr };
                Ok(MapFuture {
                    inner: future,
                    args: Some((self.fun, p))
                })
            }
            Err((transport, addr)) => Err((Map { transport, fun: self.fun }, addr)),
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

/// Custom `Stream` implementation to avoid boxing.
///
/// Maps a function over every stream item.
#[derive(Clone, Debug)]
pub struct MapStream<T, F> { stream: T, listen_addr: Multiaddr, fun: F }

impl<T, F, A, B, X> Stream for MapStream<T, F>
where
    T: Stream<Item = (X, Multiaddr)>,
    X: Future<Item = A>,
    F: FnOnce(A, ConnectedPoint) -> B + Clone
{
    type Item = (MapFuture<X, F>, Multiaddr);
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.stream.poll()? {
            Async::Ready(Some((future, addr))) => {
                let f = self.fun.clone();
                let p = ConnectedPoint::Listener {
                    listen_addr: self.listen_addr.clone(),
                    send_back_addr: addr.clone()
                };
                let future = MapFuture {
                    inner: future,
                    args: Some((f, p))
                };
                Ok(Async::Ready(Some((future, addr))))
            }
            Async::Ready(None) => Ok(Async::Ready(None)),
            Async::NotReady => Ok(Async::NotReady)
        }
    }
}

/// Custom `Future` to avoid boxing.
///
/// Applies a function to the inner future's result.
#[derive(Clone, Debug)]
pub struct MapFuture<T, F> {
    inner: T,
    args: Option<(F, ConnectedPoint)>
}

impl<T, A, F, B> Future for MapFuture<T, F>
where
    T: Future<Item = A>,
    F: FnOnce(A, ConnectedPoint) -> B
{
    type Item = B;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.inner.poll());
        let (f, a) = self.args.take().expect("MapFuture has already finished.");
        Ok(Async::Ready(f(item, a)))
    }
}

