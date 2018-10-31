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

pub mod and_then;
pub mod denied;
pub mod error;
pub mod map;
pub mod memory;
pub mod or;
pub mod or_else;
pub mod refused;
pub mod then;
pub mod upgrade;

pub use self::error::Error;
pub use self::or::{OrDialer, OrListener};
use crate::upgrade::{InboundUpgrade, OutboundUpgrade};
use futures::prelude::*;
use multiaddr::Multiaddr;
use self::{
    and_then::{AndThenDialer, AndThenListener},
    denied::{DeniedDialer, DeniedListener},
    map::{MapDialer, MapErrDialer, MapListener, MapErrListener},
    or_else::{OrElseDialer, OrElseListener},
    then::{ThenDialer, ThenListener},
    upgrade::{DialerUpgrade, ListenerUpgrade}
};
use tokio_io::{AsyncRead, AsyncWrite};

pub trait Listener {
    type Output;
    type Error;
    type Inbound: Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error>;
    type Upgrade: Future<Item = Self::Output, Error = Self::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)>
    where
        Self: Sized;

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr>;

    fn or_listener<T>(self, other: T) -> OrListener<Self, T>
    where
        Self: Sized
    {
        OrListener::new(self, other)
    }

    fn map_listener_output<F, T>(self, f: F) -> MapListener<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static
    {
        MapListener::new(self, f)
    }

    fn map_listener_error<F, E>(self, f: F) -> MapErrListener<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> E + Clone + 'static
    {
        MapErrListener::new(self, f)
    }

    fn listener_and_then<F, T>(self, f: F) -> AndThenListener<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Error = Self::Error> + 'static,
        T::Future: Send + 'static
    {
        AndThenListener::new(self, f)
    }

    fn listener_or_else<F, T>(self, f: F) -> OrElseListener<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Item = Self::Output> + 'static,
        T::Future: Send + 'static
    {
        OrElseListener::new(self, f)
    }

    fn listener_then<F, T>(self, f: F) -> ThenListener<Self, F>
    where
        Self: Sized,
        F: FnOnce(Result<(Self::Output, Multiaddr), Self::Error>) -> T + Clone + 'static,
        T: IntoFuture + 'static,
        T::Future: Send + 'static
    {
        ThenListener::new(self, f)
    }

    fn with_listener_upgrade<U>(self, upgrade: U) -> ListenerUpgrade<Self, U>
    where
        Self: Sized,
        Self::Inbound: Send,
        Self::Upgrade: Send,
        Self::Output: AsyncRead + AsyncWrite + Send,
        U: InboundUpgrade<Self::Output> + Clone + Send + 'static,
        U::NamesIter: Send + Clone,
        U::UpgradeId: Send,
        U::Future: Send
    {
        ListenerUpgrade::new(self, upgrade)
    }
}

pub trait Dialer {
    type Output;
    type Error;
    type Outbound: Future<Item = Self::Output, Error = Self::Error>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)>
    where
        Self: Sized;

    fn or_dialer<T>(self, other: T) -> OrDialer<Self, T>
    where
        Self: Sized
    {
        OrDialer::new(self, other)
    }

    fn map_dialer_output<F, T>(self, f: F) -> MapDialer<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static
    {
        MapDialer::new(self, f)
    }

    fn map_dialer_error<F, E>(self, f: F) -> MapErrDialer<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> E + Clone + 'static
    {
        MapErrDialer::new(self, f)
    }

    fn dialer_and_then<F, T>(self, f: F) -> AndThenDialer<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Error = Self::Error> + 'static,
        T::Future: Send + 'static
    {
        AndThenDialer::new(self, f)
    }

    fn dialer_or_else<F, T>(self, f: F) -> OrElseDialer<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Item = Self::Output> + 'static,
        T::Future: Send + 'static
    {
        OrElseDialer::new(self, f)
    }

    fn dialer_then<F, T>(self, f: F) -> ThenDialer<Self, F>
    where
        Self: Sized,
        F: FnOnce(Result<(Self::Output, Multiaddr), Self::Error>) -> T + Clone + 'static,
        T: IntoFuture + 'static,
        T::Future: Send + 'static
    {
        ThenDialer::new(self, f)
    }

    fn with_dialer_upgrade<U>(self, upgrade: U) -> DialerUpgrade<Self, U>
    where
        Self: Sized,
        Self::Outbound: Send,
        Self::Output: AsyncRead + AsyncWrite + Send,
        U: OutboundUpgrade<Self::Output> + Clone + Send + 'static,
        U::NamesIter: Send + Clone,
        U::UpgradeId: Send,
        U::Future: Send
    {
        DialerUpgrade::new(self, upgrade)
    }
}

#[derive(Debug, Clone)]
pub struct Transport<D, L> { dialer: D, listener: L }

impl<D, L> Transport<D, L>
where
    D: Dialer,
    L: Listener
{
    pub fn new(dialer: D, listener: L) -> Self {
        Transport { dialer, listener }
    }

    pub fn into(self) -> (D, L) {
        (self.dialer, self.listener)
    }
}

impl<L> Transport<DeniedDialer, L>
where
    L: Listener
{
    pub fn listener(listener: L) -> Self {
        Transport::new(DeniedDialer, listener)
    }

    pub fn with_dialer<D>(self, dialer: D) -> Transport<D, L>
    where
        D: Dialer
    {
        Transport { dialer, listener: self.listener }
    }
}

impl<D> Transport<D, DeniedListener>
where
    D: Dialer
{
    pub fn dialer(dialer: D) -> Self {
        Transport::new(dialer, DeniedListener)
    }

    pub fn with_listener<L>(self, listener: L) -> Transport<D, L>
    where
        L: Listener
    {
        Transport { dialer: self.dialer, listener }
    }
}

impl<D, L> Dialer for Transport<D, L>
where
    D: Dialer
{
    type Output = D::Output;
    type Error = D::Error;
    type Outbound = D::Outbound;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        let dialer = self.dialer;
        let listener = self.listener;
        dialer.dial(addr)
            .map_err(move |(dialer, addr)| (Transport { dialer, listener }, addr))
    }
}

impl<D, L> Listener for Transport<D, L>
where
    L: Listener
{
    type Output = L::Output;
    type Error = L::Error;
    type Inbound = L::Inbound;
    type Upgrade = L::Upgrade;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        let dialer = self.dialer;
        let listener = self.listener;
        listener.listen_on(addr)
            .map_err(move |(listener, addr)| (Transport { dialer, listener }, addr))
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.listener.nat_traversal(server, observed)
    }
}
