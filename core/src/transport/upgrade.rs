// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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
use crate::{either::EitherOutput, transport::Transport, upgrade::{Upgrade, apply_inbound, apply_outbound}};
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Copy, Clone)]
pub struct UpgradeTransport<T, U> { inner: T, upgrade: U }

impl<T, U> UpgradeTransport<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        UpgradeTransport { inner, upgrade }
    }
}

impl<D, U, O, E> Transport for UpgradeTransport<D, U>
where
    D: Transport,
    D::Dial: Send + 'static,
    D::Listener: Send + 'static,
    D::ListenerUpgrade: Send + 'static,
    D::Output: AsyncRead + AsyncWrite + Send + 'static,
    U: Upgrade<D::Output, Output = O, Error = E> + Send + 'static + Clone,
    U::NamesIter: Send,
    U::UpgradeId: Send,
    U::Future: Send,
    E: std::error::Error + Send + Sync + 'static
{
    type Output = O;
    type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = std::io::Error> + Send>;
    type ListenerUpgrade = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;
    type Dial = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.dial(addr.clone()) {
            Ok(outbound) =>
                Ok(Box::new(outbound.and_then(move |x| {
                    apply_outbound(x, upgrade).map_err(|e| e.into_io_error())
                }))),
            Err((dialer, addr)) => Err((UpgradeTransport::new(dialer, upgrade), addr))
        }
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.listen_on(addr) {
            Ok((inbound, addr)) => {
                let stream = inbound
                    .map(move |(future, addr)| {
                        let upgrade = upgrade.clone();
                        let future = future.and_then(move |x| {
                            apply_inbound(x, upgrade).map_err(|e| e.into_io_error())
                        });
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((UpgradeTransport::new(listener, upgrade), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct UpgradeDialer<T, U> { inner: T, upgrade: U }

impl<T, U> UpgradeDialer<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        UpgradeDialer { inner, upgrade }
    }
}

impl<D, U, O, E> Transport for UpgradeDialer<D, U>
where
    D: Transport,
    D::Dial: Send + 'static,
    D::Listener: Send + 'static,
    D::ListenerUpgrade: Send + 'static,
    D::Output: AsyncRead + AsyncWrite + Send + 'static,
    U: Upgrade<D::Output, Output = O, Error = E> + Send + 'static,
    U::NamesIter: Send,
    U::UpgradeId: Send,
    U::Future: Send,
    O: 'static,
    E: std::error::Error + Send + Sync + 'static
{
    type Output = EitherOutput<O, D::Output>;
    type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = std::io::Error> + Send>;
    type ListenerUpgrade = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;
    type Dial = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.dial(addr.clone()) {
            Ok(outbound) => {
                let future = outbound
                    .and_then(move |x| {
                        apply_outbound(x, upgrade).map_err(|e| e.into_io_error())
                    })
                    .map(EitherOutput::First);
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((UpgradeDialer::new(dialer, upgrade), addr))
        }
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.listen_on(addr) {
            Ok((inbound, addr)) => {
                let stream = inbound.map(move |(future, addr)| {
                    let future = future.map(EitherOutput::Second);
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((UpgradeDialer::new(listener, upgrade), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct UpgradeListener<T, U> { inner: T, upgrade: U }

impl<T, U> UpgradeListener<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        UpgradeListener { inner, upgrade }
    }
}

impl<D, U, O, E> Transport for UpgradeListener<D, U>
where
    D: Transport,
    D::Dial: Send + 'static,
    D::Listener: Send + 'static,
    D::ListenerUpgrade: Send + 'static,
    D::Output: AsyncRead + AsyncWrite + Send + 'static,
    U: Upgrade<D::Output, Output = O, Error = E> + Send + 'static + Clone,
    U::NamesIter: Send,
    U::UpgradeId: Send,
    U::Future: Send,
    O: 'static,
    E: std::error::Error + Send + Sync + 'static
{
    type Output = EitherOutput<D::Output, O>;
    type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = std::io::Error> + Send>;
    type ListenerUpgrade = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;
    type Dial = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.dial(addr.clone()) {
            Ok(outbound) => {
                let future = outbound.map(EitherOutput::First);
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((UpgradeListener::new(dialer, upgrade), addr))
        }
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.listen_on(addr) {
            Ok((inbound, addr)) => {
                let stream = inbound
                    .map(move |(future, addr)| {
                        let upgrade = upgrade.clone();
                        let future = future
                            .and_then(move |x| {
                                apply_inbound(x, upgrade).map_err(|e| e.into_io_error())
                            })
                            .map(EitherOutput::Second);
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((UpgradeListener::new(listener, upgrade), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}

