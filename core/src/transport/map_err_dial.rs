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
use transport::{MuxedTransport, Transport, ListenerResult, DialResult};

/// See `Transport::map_err_dial`.
#[derive(Debug, Copy, Clone)]
pub struct MapErrDial<T, F> {
    transport: T,
    map: F,
}

impl<T, F> MapErrDial<T, F> {
    /// Internal function that builds a `MapErrDial`.
    #[inline]
    pub(crate) fn new(transport: T, map: F) -> MapErrDial<T, F> {
        MapErrDial { transport, map }
    }
}

impl<T, F> Transport for MapErrDial<T, F>
where
    T: Transport + 'static,                          // TODO: 'static :-/
    F: FnOnce(IoError, Multiaddr) -> IoError + Clone + 'static, // TODO: 'static :-/
{
    type Output = T::Output;
    type MultiaddrFuture = T::MultiaddrFuture;
    type Listener = T::Listener;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = Box<Future<Item = (Self::Output, Self::MultiaddrFuture), Error = IoError>>;

    fn listen_on(&self, addr: Multiaddr) -> ListenerResult<Self::Listener> {
        self.transport.listen_on(addr)
    }

    fn dial(&self, addr: Multiaddr) -> DialResult<Self::Dial> {
        let map = self.map.clone();
        Ok(Box::new(self.transport.dial(addr.clone())?
            .into_future()
            .map_err(move |err| map(err, addr))))
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

impl<T, F> MuxedTransport for MapErrDial<T, F>
where
    T: MuxedTransport + 'static,                     // TODO: 'static :-/
    F: FnOnce(IoError, Multiaddr) -> IoError + Clone + 'static, // TODO: 'static :-/
{
    type Incoming = T::Incoming;
    type IncomingUpgrade = T::IncomingUpgrade;

    #[inline]
    fn next_incoming(self) -> Self::Incoming {
        self.transport.next_incoming()
    }
}
