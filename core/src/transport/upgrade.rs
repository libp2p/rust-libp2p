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

use crate::{
    transport::Transport,
    upgrade::{
        OutboundUpgrade,
        InboundUpgrade,
        apply_inbound,
        apply_outbound,
        UpgradeError,
        OutboundUpgradeApply,
        InboundUpgradeApply
    }
};
use futures::{future::Either, prelude::*, try_ready};
use multiaddr::Multiaddr;
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Copy, Clone)]
pub struct Upgrade<T, U> { inner: T, upgrade: U }

impl<T, U> Upgrade<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        Upgrade { inner, upgrade }
    }
}

impl<D, U, O, E> Transport for Upgrade<D, U>
where
    D: Transport,
    D::Output: AsyncRead + AsyncWrite,
    U: InboundUpgrade<D::Output, Output = O, Error = E>,
    U: OutboundUpgrade<D::Output, Output = O, Error = E> + Clone,
    E: std::error::Error + Send + Sync + 'static
{
    type Output = O;
    type Listener = ListenerStream<D::Listener, U>;
    type ListenerUpgrade = ListenerUpgradeFuture<D::ListenerUpgrade, U>;
    type Dial = DialUpgradeFuture<D::Dial, U>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match self.inner.dial(addr.clone()) {
            Ok(outbound) => Ok(DialUpgradeFuture {
                future: outbound,
                upgrade: Either::A(Some(self.upgrade))
            }),
            Err((dialer, addr)) => Err((Upgrade::new(dialer, self.upgrade), addr))
        }
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        match self.inner.listen_on(addr) {
            Ok((inbound, addr)) =>
                Ok((ListenerStream { stream: inbound, upgrade: self.upgrade }, addr)),
            Err((listener, addr)) =>
                Err((Upgrade::new(listener, self.upgrade), addr))
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}

pub struct DialUpgradeFuture<T, U>
where
    T: Future,
    T::Item: AsyncRead + AsyncWrite,
    U: OutboundUpgrade<T::Item>
{
    future: T,
    upgrade: Either<Option<U>, OutboundUpgradeApply<T::Item, U>>
}

impl<T, U> Future for DialUpgradeFuture<T, U>
where
    T: Future<Error = std::io::Error>,
    T::Item: AsyncRead + AsyncWrite,
    U: OutboundUpgrade<T::Item>,
    U::Error: std::error::Error + Send + Sync + 'static
{
    type Item = U::Output;
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let next = match self.upgrade {
                Either::A(ref mut up) => {
                    let x = try_ready!(self.future.poll());
                    let u = up.take().expect("DialUpgradeFuture is constructed with Either::A(Some).");
                    Either::B(apply_outbound(x, u))
                }
                Either::B(ref mut up) => return up.poll().map_err(UpgradeError::into_io_error)
            };
            self.upgrade = next
        }
    }
}

pub struct ListenerStream<T, U> {
    stream: T,
    upgrade: U
}

impl<T, U, F> Stream for ListenerStream<T, U>
where
    T: Stream<Item = (F, Multiaddr)>,
    F: Future,
    F::Item: AsyncRead + AsyncWrite,
    U: InboundUpgrade<F::Item> + Clone
{
    type Item = (ListenerUpgradeFuture<F, U>, Multiaddr);
    type Error = T::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match try_ready!(self.stream.poll()) {
            Some((x, a)) => {
                let f = ListenerUpgradeFuture {
                    future: x,
                    upgrade: Either::A(Some(self.upgrade.clone()))
                };
                Ok(Async::Ready(Some((f, a))))
            }
            None => Ok(Async::Ready(None))
        }
    }
}

pub struct ListenerUpgradeFuture<T, U>
where
    T: Future,
    T::Item: AsyncRead + AsyncWrite,
    U: InboundUpgrade<T::Item>
{
    future: T,
    upgrade: Either<Option<U>, InboundUpgradeApply<T::Item, U>>
}

impl<T, U> Future for ListenerUpgradeFuture<T, U>
where
    T: Future<Error = std::io::Error>,
    T::Item: AsyncRead + AsyncWrite,
    U: InboundUpgrade<T::Item>,
    U::Error: std::error::Error + Send + Sync + 'static
{
    type Item = U::Output;
    type Error = std::io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let next = match self.upgrade {
                Either::A(ref mut up) => {
                    let x = try_ready!(self.future.poll());
                    let u = up.take().expect("ListenerUpgradeFuture is constructed with Either::A(Some).");
                    Either::B(apply_inbound(x, u))
                }
                Either::B(ref mut up) => return up.poll().map_err(UpgradeError::into_io_error)
            };
            self.upgrade = next
        }
    }
}

