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
    connection::{ConnectedPoint, Endpoint},
    either::EitherError,
    transport::{ListenerEvent, Transport, TransportError},
};
use futures::{future::Either, prelude::*};
use multiaddr::Multiaddr;
use std::{error, marker::PhantomPinned, pin::Pin, task::Context, task::Poll};

/// See the `Transport::and_then` method.
#[derive(Debug, Clone)]
pub struct AndThen<T, C> {
    transport: T,
    fun: C,
}

impl<T, C> AndThen<T, C> {
    pub(crate) fn new(transport: T, fun: C) -> Self {
        AndThen { transport, fun }
    }
}

impl<T, C, F, O> Transport for AndThen<T, C>
where
    T: Transport,
    C: FnOnce(T::Output, ConnectedPoint) -> F + Clone,
    F: TryFuture<Ok = O>,
    F::Error: error::Error,
{
    type Output = O;
    type Error = EitherError<T::Error, F::Error>;
    type Listener = AndThenStream<T::Listener, C>;
    type ListenerUpgrade = AndThenFuture<T::ListenerUpgrade, C, F>;
    type Dial = AndThenFuture<T::Dial, C, F>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let listener = self
            .transport
            .listen_on(addr)
            .map_err(|err| err.map(EitherError::A))?;
        // Try to negotiate the protocol.
        // Note that failing to negotiate a protocol will never produce a future with an error.
        // Instead the `stream` will produce `Ok(Err(...))`.
        // `stream` can only produce an `Err` if `listening_stream` produces an `Err`.
        let stream = AndThenStream {
            stream: listener,
            fun: self.fun,
        };
        Ok(stream)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dialed_fut = self
            .transport
            .dial(addr.clone())
            .map_err(|err| err.map(EitherError::A))?;
        let future = AndThenFuture {
            inner: Either::Left(Box::pin(dialed_fut)),
            args: Some((
                self.fun,
                ConnectedPoint::Dialer {
                    address: addr,
                    role_override: Endpoint::Dialer,
                },
            )),
            _marker: PhantomPinned,
        };
        Ok(future)
    }

    fn dial_as_listener(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dialed_fut = self
            .transport
            .dial_as_listener(addr.clone())
            .map_err(|err| err.map(EitherError::A))?;
        let future = AndThenFuture {
            inner: Either::Left(Box::pin(dialed_fut)),
            args: Some((
                self.fun,
                ConnectedPoint::Dialer {
                    address: addr,
                    role_override: Endpoint::Listener,
                },
            )),
            _marker: PhantomPinned,
        };
        Ok(future)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.address_translation(server, observed)
    }
}

/// Custom `Stream` to avoid boxing.
///
/// Applies a function to every stream item.
#[pin_project::pin_project]
#[derive(Debug, Clone)]
pub struct AndThenStream<TListener, TMap> {
    #[pin]
    stream: TListener,
    fun: TMap,
}

impl<TListener, TMap, TTransOut, TMapOut, TListUpgr, TTransErr> Stream
    for AndThenStream<TListener, TMap>
where
    TListener: TryStream<Ok = ListenerEvent<TListUpgr, TTransErr>, Error = TTransErr>,
    TListUpgr: TryFuture<Ok = TTransOut, Error = TTransErr>,
    TMap: FnOnce(TTransOut, ConnectedPoint) -> TMapOut + Clone,
    TMapOut: TryFuture,
{
    type Item = Result<
        ListenerEvent<
            AndThenFuture<TListUpgr, TMap, TMapOut>,
            EitherError<TTransErr, TMapOut::Error>,
        >,
        EitherError<TTransErr, TMapOut::Error>,
    >;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match TryStream::try_poll_next(this.stream, cx) {
            Poll::Ready(Some(Ok(event))) => {
                let event = match event {
                    ListenerEvent::Upgrade {
                        upgrade,
                        local_addr,
                        remote_addr,
                    } => {
                        let point = ConnectedPoint::Listener {
                            local_addr: local_addr.clone(),
                            send_back_addr: remote_addr.clone(),
                        };
                        ListenerEvent::Upgrade {
                            upgrade: AndThenFuture {
                                inner: Either::Left(Box::pin(upgrade)),
                                args: Some((this.fun.clone(), point)),
                                _marker: PhantomPinned,
                            },
                            local_addr,
                            remote_addr,
                        }
                    }
                    ListenerEvent::NewAddress(a) => ListenerEvent::NewAddress(a),
                    ListenerEvent::AddressExpired(a) => ListenerEvent::AddressExpired(a),
                    ListenerEvent::Error(e) => ListenerEvent::Error(EitherError::A(e)),
                };

                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(EitherError::A(err)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Custom `Future` to avoid boxing.
///
/// Applies a function to the result of the inner future.
#[derive(Debug)]
pub struct AndThenFuture<TFut, TMap, TMapOut> {
    inner: Either<Pin<Box<TFut>>, Pin<Box<TMapOut>>>,
    args: Option<(TMap, ConnectedPoint)>,
    _marker: PhantomPinned,
}

impl<TFut, TMap, TMapOut> Future for AndThenFuture<TFut, TMap, TMapOut>
where
    TFut: TryFuture,
    TMap: FnOnce(TFut::Ok, ConnectedPoint) -> TMapOut,
    TMapOut: TryFuture,
{
    type Output = Result<TMapOut::Ok, EitherError<TFut::Error, TMapOut::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let future = match &mut self.inner {
                Either::Left(future) => {
                    let item = match TryFuture::try_poll(future.as_mut(), cx) {
                        Poll::Ready(Ok(v)) => v,
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(EitherError::A(err))),
                        Poll::Pending => return Poll::Pending,
                    };
                    let (f, a) = self
                        .args
                        .take()
                        .expect("AndThenFuture has already finished.");
                    f(item, a)
                }
                Either::Right(future) => {
                    return match TryFuture::try_poll(future.as_mut(), cx) {
                        Poll::Ready(Ok(v)) => Poll::Ready(Ok(v)),
                        Poll::Ready(Err(err)) => return Poll::Ready(Err(EitherError::B(err))),
                        Poll::Pending => Poll::Pending,
                    }
                }
            };

            self.inner = Either::Right(Box::pin(future));
        }
    }
}

impl<TFut, TMap, TMapOut> Unpin for AndThenFuture<TFut, TMap, TMapOut> {}
