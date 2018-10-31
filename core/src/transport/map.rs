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
pub struct MapDialer<D, F> { dialer: D, fun: F }

impl<D, F> MapDialer<D, F> {
    pub fn new(dialer: D, fun: F) -> Self {
        MapDialer { dialer, fun }
    }
}

impl<D, F, T> Dialer for MapDialer<D, F>
where
    D: Dialer + 'static,
    D::Outbound: Send,
    F: FnOnce(D::Output, Multiaddr) -> T + Clone + Send + 'static,
{
    type Output = T;
    type Error = D::Error;
    type Outbound = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        let fun = self.fun;
        match self.dialer.dial(addr.clone()) {
            Ok(future) => {
                let future = future.map(move |x| fun(x, addr));
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((MapDialer::new(dialer, fun), addr))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MapErrDialer<D, F> { dialer: D, fun: F }

impl<D, F> MapErrDialer<D, F> {
    pub fn new(dialer: D, fun: F) -> Self {
        MapErrDialer { dialer, fun }
    }
}

impl<D, F, E> Dialer for MapErrDialer<D, F>
where
    D: Dialer + 'static,
    D::Outbound: Send,
    F: FnOnce(D::Error, Multiaddr) -> E + Clone + Send + 'static
{
    type Output = D::Output;
    type Error = E;
    type Outbound = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        let fun = self.fun;
        match self.dialer.dial(addr.clone()) {
            Ok(future) => {
                let future = future.map_err(move |e| fun(e, addr));
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((MapErrDialer::new(dialer, fun), addr))
        }
    }
}


#[derive(Debug, Copy, Clone)]
pub struct MapListener<L, F> { listener: L, fun: F }

impl<L, F> MapListener<L, F> {
    pub fn new(listener: L, fun: F) -> Self {
        MapListener { listener, fun }
    }
}

impl<L, F, T> Listener for MapListener<L, F>
where
    L: Listener + 'static,
    L::Inbound: Send,
    L::Upgrade: Send,
    F: FnOnce(L::Output, Multiaddr) -> T + Clone + Send + 'static,
{
    type Output = T;
    type Error = L::Error;
    type Inbound = Box<Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error> + Send>;
    type Upgrade = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        let fun = self.fun;
        match self.listener.listen_on(addr) {
            Ok((stream, addr)) => {
                let stream = stream.map(move |(future, addr)| {
                    let f = fun.clone();
                    let a = addr.clone();
                    let future = future.map(move |x| f(x, a));
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((MapListener::new(listener, fun), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.listener.nat_traversal(server, observed)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MapErrListener<L, F> { listener: L, fun: F }

impl<L, F> MapErrListener<L, F> {
    pub fn new(listener: L, fun: F) -> Self {
        MapErrListener { listener, fun }
    }
}

impl<L, F, E> Listener for MapErrListener<L, F>
where
    L: Listener + 'static,
    L::Inbound: Send,
    L::Upgrade: Send,
    F: FnOnce(L::Error, Multiaddr) -> E + Clone + Send + 'static
{
    type Output = L::Output;
    type Error = E;
    type Inbound = Box<Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error> + Send>;
    type Upgrade = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        let fun = self.fun;
        match self.listener.listen_on(addr) {
            Ok((stream, addr)) => {
                let stream = stream.map(move |(future, addr)| {
                    let f = fun.clone();
                    let a = addr.clone();
                    let future = future.map_err(move |e| f(e, a));
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((MapErrListener::new(listener, fun), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.listener.nat_traversal(server, observed)
    }
}

