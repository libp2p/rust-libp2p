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
    transport::{ListenerId, Transport, TransportError, TransportEvent},
};
use futures::{future::Either, prelude::*};
use multiaddr::Multiaddr;
use std::{error, marker::PhantomPinned, pin::Pin, task::Context, task::Poll};

/// See the [`Transport::and_then`] method.
#[pin_project::pin_project]
#[derive(Debug, Clone)]
pub struct AndThen<T, C> {
    #[pin]
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
    type ListenerUpgrade = AndThenFuture<T::ListenerUpgrade, C, F>;
    type Dial = AndThenFuture<T::Dial, C, F>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        self.transport
            .listen_on(addr)
            .map_err(|err| err.map(EitherError::A))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.transport.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dialed_fut = self
            .transport
            .dial(addr.clone())
            .map_err(|err| err.map(EitherError::A))?;
        let future = AndThenFuture {
            inner: Either::Left(Box::pin(dialed_fut)),
            args: Some((
                self.fun.clone(),
                ConnectedPoint::Dialer {
                    address: addr,
                    role_override: Endpoint::Dialer,
                },
            )),
            _marker: PhantomPinned,
        };
        Ok(future)
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dialed_fut = self
            .transport
            .dial_as_listener(addr.clone())
            .map_err(|err| err.map(EitherError::A))?;
        let future = AndThenFuture {
            inner: Either::Left(Box::pin(dialed_fut)),
            args: Some((
                self.fun.clone(),
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

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        match this.transport.poll(cx) {
            Poll::Ready(TransportEvent::Incoming {
                listener_id,
                upgrade,
                local_addr,
                send_back_addr,
            }) => {
                let point = ConnectedPoint::Listener {
                    local_addr: local_addr.clone(),
                    send_back_addr: send_back_addr.clone(),
                };
                Poll::Ready(TransportEvent::Incoming {
                    listener_id,
                    upgrade: AndThenFuture {
                        inner: Either::Left(Box::pin(upgrade)),
                        args: Some((this.fun.clone(), point)),
                        _marker: PhantomPinned,
                    },
                    local_addr,
                    send_back_addr,
                })
            }
            Poll::Ready(other) => {
                let mapped = other
                    .map_upgrade(|_upgrade| unreachable!("case already matched"))
                    .map_err(EitherError::A);
                Poll::Ready(mapped)
            }
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
