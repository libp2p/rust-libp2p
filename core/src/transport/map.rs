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

use crate::{Endpoint, transport::{Transport, TransportError, ListenerEvent}};
use futures::{prelude::*, try_ready};
use multiaddr::Multiaddr;

/// See `Transport::map`.
#[derive(Debug, Copy, Clone)]
pub struct Map<T, F> { transport: T, fun: F }

impl<T, F> Map<T, F> {
    pub(crate) fn new(transport: T, fun: F) -> Self {
        Map { transport, fun }
    }
}

impl<T, F, D> Transport for Map<T, F>
where
    T: Transport,
    F: FnOnce(T::Output, Endpoint) -> D + Clone
{
    type Output = D;
    type Error = T::Error;
    type Listener = MapStream<T::Listener, F>;
    type ListenerUpgrade = MapFuture<T::ListenerUpgrade, F>;
    type Dial = MapFuture<T::Dial, F>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let stream = self.transport.listen_on(addr)?;
        Ok(MapStream { stream, fun: self.fun })
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let future = self.transport.dial(addr.clone())?;
        Ok(MapFuture { inner: future, args: Some((self.fun, Endpoint::Dialer)) })
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

/// Custom `Stream` implementation to avoid boxing.
///
/// Maps a function over every stream item.
#[derive(Clone, Debug)]
pub struct MapStream<T, F> {
    stream: T,
    fun: F
}

impl<T, F, A, B, X> Stream for MapStream<T, F>
where
    T: Stream<Item = ListenerEvent<X>>,
    X: Future<Item = A>,
    F: FnOnce(A, Endpoint) -> B + Clone
{
    type Item = ListenerEvent<MapFuture<X, F>>;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.stream.poll()? {
            Async::Ready(Some(event)) => {
                let event = event.map(|future| {
                    MapFuture {
                        inner: future,
                        args: Some((self.fun.clone(), Endpoint::Listener))
                    }
                });
                Ok(Async::Ready(Some(event)))
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
    args: Option<(F, Endpoint)>
}

impl<T, A, F, B> Future for MapFuture<T, F>
where
    T: Future<Item = A>,
    F: FnOnce(A, Endpoint) -> B
{
    type Item = B;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.inner.poll());
        let (f, a) = self.args.take().expect("MapFuture has already finished.");
        Ok(Async::Ready(f(item, a)))
    }
}

