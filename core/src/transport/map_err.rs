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

use crate::transport::{Transport, TransportError, ListenerEvent};
use futures::prelude::*;
use multiaddr::Multiaddr;
use std::error;

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

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let map = self.map;
        match self.transport.listen_on(addr) {
            Ok(stream) => Ok(MapErrListener { inner: stream, map }),
            Err(err) => Err(err.map(map))
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let map = self.map;
        match self.transport.dial(addr) {
            Ok(future) => Ok(MapErrDial { inner: future, map: Some(map) }),
            Err(err) => Err(err.map(map)),
        }
    }
}

/// Listening stream for `MapErr`.
pub struct MapErrListener<T: Transport, F> {
    inner: T::Listener,
    map: F,
}

impl<T, F, TErr> Stream for MapErrListener<T, F>
where
    T: Transport,
    F: FnOnce(T::Error) -> TErr + Clone,
    TErr: error::Error,
{
    type Item = ListenerEvent<MapErrListenerUpgrade<T, F>>;
    type Error = TErr;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(Some(event))) => {
                let event = event.map(move |value| {
                    MapErrListenerUpgrade {
                        inner: value,
                        map: Some(self.map.clone())
                    }
                });
                Ok(Async::Ready(Some(event)))
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => Err((self.map.clone())(err)),
        }
    }
}

/// Listening upgrade future for `MapErr`.
pub struct MapErrListenerUpgrade<T: Transport, F> {
    inner: T::ListenerUpgrade,
    map: Option<F>,
}

impl<T, F, TErr> Future for MapErrListenerUpgrade<T, F>
where T: Transport,
    F: FnOnce(T::Error) -> TErr,
{
    type Item = T::Output;
    type Error = TErr;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(value)) => Ok(Async::Ready(value)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                let map = self.map.take().expect("poll() called again after error");
                Err(map(err))
            }
        }
    }
}

/// Dialing future for `MapErr`.
pub struct MapErrDial<T: Transport, F> {
    inner: T::Dial,
    map: Option<F>,
}

impl<T, F, TErr> Future for MapErrDial<T, F>
where
    T: Transport,
    F: FnOnce(T::Error) -> TErr,
{
    type Item = T::Output;
    type Error = TErr;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(value)) => {
                Ok(Async::Ready(value))
            },
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                let map = self.map.take().expect("poll() called again after error");
                Err(map(err))
            }
        }
    }
}
