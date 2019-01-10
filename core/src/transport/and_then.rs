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

use crate::either::EitherError;
use crate::{nodes::raw_swarm::ConnectedPoint, transport::Transport, transport::TransportError};
use futures::{future::Either, prelude::*, try_ready};
use multiaddr::Multiaddr;
use std::error;

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
    F: IntoFuture<Item = O>,
    F::Error: error::Error,
{
    type Output = O;
    type Error = EitherError<T::Error, F::Error>;
    type Listener = AndThenStream<T::Listener, C>;
    type ListenerUpgrade = AndThenFuture<T::ListenerUpgrade, C, F::Future>;
    type Dial = AndThenFuture<T::Dial, C, F::Future>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        let (listening_stream, new_addr) = self.transport.listen_on(addr)
            .map_err(|err| err.map(EitherError::A))?;

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
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dialed_fut = self.transport.dial(addr.clone())
            .map_err(|err| err.map(EitherError::A))?;

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
pub struct AndThenStream<TListener, TMap> { stream: TListener, listen_addr: Multiaddr, fun: TMap }

impl<TListener, TMap, TTransOut, TMapOut, TListUpgr, TTransErr> Stream for AndThenStream<TListener, TMap>
where
    TListener: Stream<Item = (TListUpgr, Multiaddr), Error = TTransErr>,
    TListUpgr: Future<Item = TTransOut, Error = TTransErr>,
    TMap: FnOnce(TTransOut, ConnectedPoint) -> TMapOut + Clone,
    TMapOut: IntoFuture
{
    type Item = (AndThenFuture<TListUpgr, TMap, TMapOut::Future>, Multiaddr);
    type Error = EitherError<TTransErr, TMapOut::Error>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.stream.poll().map_err(EitherError::A)? {
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
pub struct AndThenFuture<TFut, TMap, TMapOut> {
    inner: Either<TFut, TMapOut>,
    args: Option<(TMap, ConnectedPoint)>
}

impl<TFut, TMap, TMapOut> Future for AndThenFuture<TFut, TMap, TMapOut::Future>
where
    TFut: Future,
    TMap: FnOnce(TFut::Item, ConnectedPoint) -> TMapOut,
    TMapOut: IntoFuture
{
    type Item = <TMapOut::Future as Future>::Item;
    type Error = EitherError<TFut::Error, TMapOut::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let future = match self.inner {
                Either::A(ref mut future) => {
                    let item = try_ready!(future.poll().map_err(EitherError::A));
                    let (f, a) = self.args.take().expect("AndThenFuture has already finished.");
                    f(item, a).into_future()
                }
                Either::B(ref mut future) => return future.poll().map_err(EitherError::B)
            };

            self.inner = Either::B(future);
        }
    }
}

