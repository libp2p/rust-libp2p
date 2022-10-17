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

use crate::transport::{ListenerId, Transport, TransportError, TransportEvent};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::{error, pin::Pin, task::Context, task::Poll};

/// See `Transport::map_err`.
#[derive(Debug, Copy, Clone)]
#[pin_project::pin_project]
pub struct MapErr<T, F> {
    #[pin]
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
    type ListenerUpgrade = MapErrListenerUpgrade<T, F>;
    type Dial = MapErrDial<T, F>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let map = self.map.clone();
        self.transport.listen_on(addr).map_err(|err| err.map(map))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.transport.remove_listener(id)
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

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.project();
        let map = &*this.map;
        this.transport.poll(cx).map(|ev| {
            ev.map_upgrade(move |value| MapErrListenerUpgrade {
                inner: value,
                map: Some(map.clone()),
            })
            .map_err(map.clone())
        })
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
