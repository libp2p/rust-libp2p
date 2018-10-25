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

use futures::prelude::*;
use multiaddr::Multiaddr;
use std::io::Error as IoError;
use transport::Transport;
use Endpoint;

/// See `Transport::map`.
#[derive(Debug, Copy, Clone)]
pub struct Map<T, F> {
    transport: T,
    map: F,
}

impl<T, F> Map<T, F> {
    /// Internal function that builds a `Map`.
    #[inline]
    pub(crate) fn new(transport: T, map: F) -> Map<T, F> {
        Map { transport, map }
    }
}

impl<T, F, D> Transport for Map<T, F>
where
    T: Transport + 'static,                  // TODO: 'static :-/
    T::Dial: Send,
    T::Listener: Send,
    T::ListenerUpgrade: Send,
    F: FnOnce(T::Output, Endpoint) -> D + Clone + Send + 'static, // TODO: 'static :-/
{
    type Output = D;
    type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = IoError> + Send>;
    type ListenerUpgrade = Box<Future<Item = Self::Output, Error = IoError> + Send>;
    type Dial = Box<Future<Item = Self::Output, Error = IoError> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let map = self.map;

        match self.transport.listen_on(addr) {
            Ok((stream, listen_addr)) => {
                let stream = stream.map(move |(future, addr)| {
                    let map = map.clone();
                    let future = future
                        .into_future()
                        .map(move |output| map(output, Endpoint::Listener));
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), listen_addr))
            }
            Err((transport, addr)) => Err((Map { transport, map }, addr)),
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let map = self.map;

        match self.transport.dial(addr) {
            Ok(future) => {
                let future = future
                    .into_future()
                    .map(move |output| map(output, Endpoint::Dialer));
                Ok(Box::new(future))
            }
            Err((transport, addr)) => Err((Map { transport, map }, addr)),
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}
