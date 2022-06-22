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

use crate::transport::{ListenerEvent, Transport, TransportError};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{error, pin::Pin, task::Context, task::Poll};

/// See `Transport::map_err`.
#[derive(Debug, Copy, Clone)]
pub struct MapErr<T, F> {
    transport: T,
    map: F,
}

impl<T, F> MapErr<T, F> {
    /// Internal function that builds a `MapErr`.
    pub(crate) fn new(transport: T, map: F) -> MapErr<T, F> {
        MapErr { transport, map }
    }
}

impl<T, F, TErr> Transport for MapErr<T, F>
where
    T: Transport,
    F: FnOnce(T::Error) -> TErr + Clone,
    TErr: error::Error,
{
    type Output = T::Output;
    type Error = TErr;
    type Listener = MapErrListener<T, F>;
    type ListenerUpgrade = MapErrListenerUpgrade<T, F>;
    type Dial = MapErrDial<T, F>;

    fn listen_on(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Listener, TransportError<Self::Error>> {
        let map = self.map.clone();
        match self.transport.listen_on(addr) {
            Ok(stream) => Ok(MapErrListener { inner: stream, map }),
            Err(err) => Err(err.map(map)),
        }
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let map = self.map.clone();
        match self.transport.dial(addr) {
            Ok(future) => Ok(MapErrDial {
                inner: future,
                map: Some(map),
            }),
            Err(err) => Err(err.map(map)),
        }
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let map = self.map.clone();
        match self.transport.dial_as_listener(addr) {
            Ok(future) => Ok(MapErrDial {
                inner: future,
                map: Some(map),
            }),
            Err(err) => Err(err.map(map)),
        }
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.address_translation(server, observed)
    }
}

/// Listening stream for `MapErr`.
#[pin_project::pin_project]
pub struct MapErrListener<T: Transport, F> {
    #[pin]
    inner: T::Listener,
    map: F,
}

impl<T, F, TErr> Stream for MapErrListener<T, F>
where
    T: Transport,
    F: FnOnce(T::Error) -> TErr + Clone,
    TErr: error::Error,
{
    type Item = Result<ListenerEvent<MapErrListenerUpgrade<T, F>, TErr>, TErr>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match TryStream::try_poll_next(this.inner, cx) {
            Poll::Ready(Some(Ok(event))) => {
                let map = &*this.map;
                let event = event
                    .map(move |value| MapErrListenerUpgrade {
                        inner: value,
                        map: Some(map.clone()),
                    })
                    .map_err(|err| (map.clone())(err));
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err((this.map.clone())(err)))),
        }
    }
}

/// Listening upgrade future for `MapErr`.
#[pin_project::pin_project]
pub struct MapErrListenerUpgrade<T: Transport, F> {
    #[pin]
    inner: T::ListenerUpgrade,
    map: Option<F>,
}

impl<T, F, TErr> Future for MapErrListenerUpgrade<T, F>
where
    T: Transport,
    F: FnOnce(T::Error) -> TErr,
{
    type Output = Result<T::Output, TErr>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match Future::poll(this.inner, cx) {
            Poll::Ready(Ok(value)) => Poll::Ready(Ok(value)),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                let map = this.map.take().expect("poll() called again after error");
                Poll::Ready(Err(map(err)))
            }
        }
    }
}

/// Dialing future for `MapErr`.
#[pin_project::pin_project]
pub struct MapErrDial<T: Transport, F> {
    #[pin]
    inner: T::Dial,
    map: Option<F>,
}

impl<T, F, TErr> Future for MapErrDial<T, F>
where
    T: Transport,
    F: FnOnce(T::Error) -> TErr,
{
    type Output = Result<T::Output, TErr>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match Future::poll(this.inner, cx) {
            Poll::Ready(Ok(value)) => Poll::Ready(Ok(value)),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => {
                let map = this.map.take().expect("poll() called again after error");
                Poll::Ready(Err(map(err)))
            }
        }
    }
}
