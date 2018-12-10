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

use futures::{future::Either, prelude::*};
use multiaddr::Multiaddr;
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
use tokio_io::{AsyncRead, AsyncWrite};

/// A possible upgrade: a `Transport` object on a connection or substream that enables transport over-the-wire via a protocol or protocols, and dialing, listening and NAT traversing a peer.
#[derive(Debug, Copy, Clone)]
pub struct Upgrade<T, U> {
    /// `Transport` object that is dialed, listened on and NAT traversed.
    inner: T,
    /// The upgrade that is returned in a wrapper on dialing or listening on a peer, 
    upgrade: U
}

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

/// Wrapper around a future that upgrades an outbound request or dial. Future that upgrades the dialing side to use an upgrade
pub struct DialUpgradeFuture<T, U>
where
    T: Future,
    T::Item: AsyncRead + AsyncWrite,
    U: OutboundUpgrade<T::Item>
{
    /// The future containing an outbound
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

/// Wrapper around a stream that upgrades a future of a listener or inbound request.
pub struct ListenerStream<T, U> {
    /// Stream containing the future (which contains the to-be-listener to upgrade) and multiaddress.
    stream: T,
    /// Used for upgrading the future to a listener.
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

/// Wrapper around a future that contains a to-be upgraded listener, or inbound connection or substream.
pub struct ListenerUpgradeFuture<T, U>
where
    T: Future,
    T::Item: AsyncRead + AsyncWrite,
    U: InboundUpgrade<T::Item>
{
    /// Future to contain the to-be upgraded listener.
    future: T,
    /// Upgrade for the listener.
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

