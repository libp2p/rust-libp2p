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

use crate::{
    connection::{ConnectedPoint, Endpoint},
    transport::{ListenerEvent, Transport, TransportError},
};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{pin::Pin, task::Context, task::Poll};

/// See `Transport::map`.
#[derive(Debug, Copy, Clone)]
pub struct Map<T, F> {
    transport: T,
    fun: F,
}

impl<T, F> Map<T, F> {
    pub(crate) fn new(transport: T, fun: F) -> Self {
        Map { transport, fun }
    }

    pub fn inner(&self) -> &T {
        &self.transport
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.transport
    }
}

impl<T, F, D> Transport for Map<T, F>
where
    T: Transport,
    F: FnOnce(T::Output, ConnectedPoint) -> D + Clone,
{
    type Output = D;
    type Error = T::Error;
    type Listener = MapStream<T::Listener, F>;
    type ListenerUpgrade = MapFuture<T::ListenerUpgrade, F>;
    type Dial = MapFuture<T::Dial, F>;

    fn listen_on(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Listener, TransportError<Self::Error>> {
        let stream = self.transport.listen_on(addr)?;
        Ok(MapStream {
            stream,
            fun: self.fun.clone(),
        })
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let future = self.transport.dial(addr.clone())?;
        let p = ConnectedPoint::Dialer {
            address: addr,
            role_override: Endpoint::Dialer,
        };
        Ok(MapFuture {
            inner: future,
            args: Some((self.fun.clone(), p)),
        })
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let future = self.transport.dial_as_listener(addr.clone())?;
        let p = ConnectedPoint::Dialer {
            address: addr,
            role_override: Endpoint::Listener,
        };
        Ok(MapFuture {
            inner: future,
            args: Some((self.fun.clone(), p)),
        })
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.address_translation(server, observed)
    }
}

/// Custom `Stream` implementation to avoid boxing.
///
/// Maps a function over every stream item.
#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapStream<T, F> {
    #[pin]
    stream: T,
    fun: F,
}

impl<T, F, A, B, X, E> Stream for MapStream<T, F>
where
    T: TryStream<Ok = ListenerEvent<X, E>, Error = E>,
    X: TryFuture<Ok = A>,
    F: FnOnce(A, ConnectedPoint) -> B + Clone,
{
    type Item = Result<ListenerEvent<MapFuture<X, F>, E>, E>;

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
                            upgrade: MapFuture {
                                inner: upgrade,
                                args: Some((this.fun.clone(), point)),
                            },
                            local_addr,
                            remote_addr,
                        }
                    }
                    ListenerEvent::NewAddress(a) => ListenerEvent::NewAddress(a),
                    ListenerEvent::AddressExpired(a) => ListenerEvent::AddressExpired(a),
                    ListenerEvent::Error(e) => ListenerEvent::Error(e),
                };
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Custom `Future` to avoid boxing.
///
/// Applies a function to the inner future's result.
#[pin_project::pin_project]
#[derive(Clone, Debug)]
pub struct MapFuture<T, F> {
    #[pin]
    inner: T,
    args: Option<(F, ConnectedPoint)>,
}

impl<T, A, F, B> Future for MapFuture<T, F>
where
    T: TryFuture<Ok = A>,
    F: FnOnce(A, ConnectedPoint) -> B,
{
    type Output = Result<B, T::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let item = match TryFuture::try_poll(this.inner, cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(v)) => v,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
        };
        let (f, a) = this.args.take().expect("MapFuture has already finished.");
        Poll::Ready(Ok(f(item, a)))
    }
}
