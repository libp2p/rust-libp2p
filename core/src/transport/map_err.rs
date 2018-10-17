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

use futures::prelude::*;
use multiaddr::Multiaddr;
use std::io::Error as IoError;
use transport::Transport;

/// See `Transport::map_err`.
#[derive(Debug, Copy, Clone)]
pub struct MapErr<T, F> {
    transport: T,
    map: F,
}

impl<T, F> MapErr<T, F> {
    /// Internal function that builds a `MapErr`.
    #[inline]
    pub(crate) fn new(transport: T, map: F) -> MapErr<T, F> {
        MapErr { transport, map }
    }
}

impl<T, F> Transport for MapErr<T, F>
where
    T: Transport,
    F: FnOnce(IoError) -> IoError + Clone,
{
    type Output = T::Output;
    type Listener = MapErrListener<T, F>;
    type ListenerUpgrade = MapErrListenerUpgrade<T, F>;
    type Dial = MapErrDial<T, F>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let map = self.map;

        match self.transport.listen_on(addr) {
            Ok((stream, listen_addr)) => {
                let stream = MapErrListener { inner: stream, map };
                Ok((stream, listen_addr))
            }
            Err((transport, addr)) => Err((MapErr { transport, map }, addr)),
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let map = self.map;

        match self.transport.dial(addr) {
            Ok(future) => Ok(MapErrDial { inner: future, map: Some(map) }),
            Err((transport, addr)) => Err((MapErr { transport, map }, addr)),
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

/// Listening stream for `MapErr`.
pub struct MapErrListener<T, F>
where T: Transport {
    inner: T::Listener,
    map: F,
}

impl<T, F> Stream for MapErrListener<T, F>
where T: Transport,
    F: FnOnce(IoError) -> IoError + Clone,
{
    type Item = (MapErrListenerUpgrade<T, F>, Multiaddr);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match try_ready!(self.inner.poll()) {
            Some((value, addr)) => Ok(Async::Ready(
                Some((MapErrListenerUpgrade { inner: value, map: Some(self.map.clone()) }, addr)))),
            None => Ok(Async::Ready(None))
        }
    }
}

/// Listening upgrade future for `MapErr`.
pub struct MapErrListenerUpgrade<T, F>
where T: Transport {
    inner: T::ListenerUpgrade,
    map: Option<F>,
}

impl<T, F> Future for MapErrListenerUpgrade<T, F>
where T: Transport,
    F: FnOnce(IoError) -> IoError,
{
    type Item = T::Output;
    type Error = IoError;

    #[inline]
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

/// Dialing future for `MapErr`.
pub struct MapErrDial<T, F>
where T: Transport,
    F: FnOnce(IoError) -> IoError,
{
    inner: T::Dial,
    map: Option<F>,
}

impl<T, F> Future for MapErrDial<T, F>
where T: Transport,
    F: FnOnce(IoError) -> IoError,
{
    type Item = T::Output;
    type Error = IoError;

    #[inline]
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
