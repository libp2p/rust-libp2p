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

use crate::{nodes::raw_swarm::ConnectedPoint, transport::Transport};
use futures::{future::Either, prelude::*, try_ready};
use multiaddr::Multiaddr;
use std::io;

/// See the `Transport::and_then` method.
#[derive(Debug, Clone)]
pub struct AndThen<T, C> { transport: T, fun: C }

impl<T, C> AndThen<T, C> {
    #[inline]
    pub(crate) fn new(transport: T, fun: C) -> Self {
        AndThen { transport, fun }
    }
}

impl<T, C, F, O> Transport for AndThen<T, C>
where
    T: Transport,
    C: FnOnce(T::Output, ConnectedPoint) -> F + Clone,
    F: IntoFuture<Item = O, Error = io::Error>
{
    type Output = O;
    type Listener = AndThenStream<T::Listener, C>;
    type ListenerUpgrade = AndThenFuture<T::ListenerUpgrade, C, F::Future>;
    type Dial = AndThenFuture<T::Dial, C, F::Future>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let (listening_stream, new_addr) = match self.transport.listen_on(addr) {
            Ok((l, new_addr)) => (l, new_addr),
            Err((transport, addr)) => {
                let builder = AndThen { transport, fun: self.fun };
                return Err((builder, addr));
            }
        };

        // Try to negotiate the protocol.
        // Note that failing to negotiate a protocol will never produce a future with an error.
        // Instead the `stream` will produce `Ok(Err(...))`.
        // `stream` can only produce an `Err` if `listening_stream` produces an `Err`.
        let stream = AndThenStream {
            stream: listening_stream,
            listen_addr: new_addr.clone(),
            fun: self.fun
        };

        Ok((stream, new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let dialed_fut = match self.transport.dial(addr.clone()) {
            Ok(f) => f,
            Err((transport, addr)) => {
                let builder = AndThen { transport, fun: self.fun };
                return Err((builder, addr));
            }
        };

        let connected_point = ConnectedPoint::Dialer { address: addr };

        let future = AndThenFuture {
            inner: Either::A(dialed_fut),
            args: Some((self.fun, connected_point))
        };

        Ok(future)
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

/// Custom `Stream` to avoid boxing.
///
/// Applies a function to every stream item.
#[derive(Debug, Clone)]
pub struct AndThenStream<T, F> { stream: T, listen_addr: Multiaddr, fun: F }

impl<T, F, A, B, X> Stream for AndThenStream<T, F>
where
    T: Stream<Item = (X, Multiaddr)>,
    X: Future<Item = A>,
    F: FnOnce(A, ConnectedPoint) -> B + Clone,
    B: IntoFuture<Error = X::Error>
{
    type Item = (AndThenFuture<X, F, B::Future>, Multiaddr);
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.stream.poll()? {
            Async::Ready(Some((future, addr))) => {
                let f = self.fun.clone();
                let p = ConnectedPoint::Listener {
                    listen_addr: self.listen_addr.clone(),
                    send_back_addr: addr.clone()
                };
                let future = AndThenFuture {
                    inner: Either::A(future),
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
/// Applies a function to the result of the inner future.
#[derive(Debug)]
pub struct AndThenFuture<T, F, U> {
    inner: Either<T, U>,
    args: Option<(F, ConnectedPoint)>
}

impl<T, A, F, B> Future for AndThenFuture<T, F, B::Future>
where
    T: Future<Item = A>,
    F: FnOnce(A, ConnectedPoint) -> B,
    B: IntoFuture<Error = T::Error>
{
    type Item = <B::Future as Future>::Item;
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let future = match self.inner {
                Either::A(ref mut future) => {
                    let item = try_ready!(future.poll());
                    let (f, a) = self.args.take().expect("AndThenFuture has already finished.");
                    f(item, a).into_future()
                }
                Either::B(ref mut future) => return future.poll()
            };

            self.inner = Either::B(future);
        }
    }
}

