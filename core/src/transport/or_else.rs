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
use crate::transport::{Dialer, Listener};

#[derive(Debug, Copy, Clone)]
pub struct OrElseDialer<D, F> { dialer: D, fun: F }

impl<D, F> OrElseDialer<D, F> {
    pub fn new(dialer: D, fun: F) -> Self {
        OrElseDialer { dialer, fun }
    }
}

impl<D, F, T> Dialer for OrElseDialer<D, F>
where
    D: Dialer + 'static,
    D::Outbound: Send,
    F: FnOnce(D::Error, Multiaddr) -> T + Clone + Send + 'static,
    T: IntoFuture<Item = D::Output> + Send + 'static,
    T::Future: Send + 'static
{
    type Output = D::Output;
    type Error = T::Error;
    type Outbound = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        let fun = self.fun;
        match self.dialer.dial(addr.clone()) {
            Ok(future) => {
                let future = future.or_else(move |x| fun(x, addr).into_future());
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((OrElseDialer::new(dialer, fun), addr))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct OrElseListener<L, F> { listener: L, fun: F }

impl<L, F> OrElseListener<L, F> {
    pub fn new(listener: L, fun: F) -> Self {
        OrElseListener { listener, fun }
    }
}

impl<L, F, T> Listener for OrElseListener<L, F>
where
    L: Listener + 'static,
    L::Inbound: Send,
    L::Upgrade: Send,
    F: FnOnce(L::Error, Multiaddr) -> T + Clone + Send + 'static,
    T: IntoFuture<Item = L::Output> + Send + 'static,
    T::Future: Send + 'static
{
    type Output = L::Output;
    type Error = T::Error;
    type Inbound = Box<Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error> + Send>;
    type Upgrade = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        let fun = self.fun;
        match self.listener.listen_on(addr) {
            Ok((stream, addr)) => {
                let stream = stream.map(move |(future, addr)| {
                    let f = fun.clone();
                    let a = addr.clone();
                    let future = future.or_else(move |x| f(x, a).into_future());
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((OrElseListener::new(listener, fun), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.listener.nat_traversal(server, observed)
    }
}
