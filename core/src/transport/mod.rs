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

mod denied;
mod error;
mod refused;

pub mod and_then;
pub mod map;
pub mod map_err;
pub mod memory;
pub mod or;
pub mod or_else;
pub mod then;
pub mod upgrade;

use crate::upgrade::{InboundUpgrade, OutboundUpgrade};
use futures::prelude::*;
use multiaddr::Multiaddr;
use self::{
    and_then::AndThen,
    map::Map,
    map_err::MapErr,
    or_else::OrElse,
    then::Then,
    upgrade::Upgrade
};
use tokio_io::{AsyncRead, AsyncWrite};

pub use self::{error::Error, denied::{DeniedDialer, DeniedListener}, or::Or};

pub trait Listener {
    type Output;
    type Error;
    type Inbound: Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error>;
    type Upgrade: Future<Item = Self::Output, Error = Self::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)>
    where
        Self: Sized;

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr>;
}

pub trait ListenerExt: Listener {
    fn or<T>(self, other: T) -> Or<Self, T>
    where
        Self: Sized
    {
        Or::new(self, other)
    }

    fn map<F, T>(self, f: F) -> Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static
    {
        Map::new(self, f)
    }

    fn map_err<F, E>(self, f: F) -> MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> E + Clone + 'static
    {
        MapErr::new(self, f)
    }

    fn and_then<F, T>(self, f: F) -> AndThen<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Error = Self::Error> + 'static,
        T::Future: Send + 'static
    {
        AndThen::new(self, f)
    }

    fn or_else<F, T>(self, f: F) -> OrElse<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Item = Self::Output> + 'static,
        T::Future: Send + 'static
    {
        OrElse::new(self, f)
    }

    fn then<F, T>(self, f: F) -> Then<Self, F>
    where
        Self: Sized,
        F: FnOnce(Result<(Self::Output, Multiaddr), Self::Error>) -> T + Clone + 'static,
        T: IntoFuture + 'static,
        T::Future: Send + 'static
    {
        Then::new(self, f)
    }

    fn with_upgrade<U>(self, upgrade: U) -> Upgrade<Self, U>
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
        Upgrade::new(self, upgrade)
    }
}

impl<L: Listener> ListenerExt for L {}

pub trait Dialer {
    type Output;
    type Error;
    type Outbound: Future<Item = Self::Output, Error = Self::Error>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> where Self: Sized;
}

pub trait DialerExt: Dialer {
    fn or<T>(self, other: T) -> Or<Self, T>
    where
        Self: Sized
    {
        Or::new(self, other)
    }

    fn map<F, T>(self, f: F) -> Map<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static
    {
        Map::new(self, f)
    }

    fn map_err<F, E>(self, f: F) -> MapErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> E + Clone + 'static
    {
        MapErr::new(self, f)
    }

    fn and_then<F, T>(self, f: F) -> AndThen<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Error = Self::Error> + 'static,
        T::Future: Send + 'static
    {
        AndThen::new(self, f)
    }

    fn or_else<F, T>(self, f: F) -> OrElse<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Item = Self::Output> + 'static,
        T::Future: Send + 'static
    {
        OrElse::new(self, f)
    }

    fn then<F, T>(self, f: F) -> Then<Self, F>
    where
        Self: Sized,
        F: FnOnce(Result<(Self::Output, Multiaddr), Self::Error>) -> T + Clone + 'static,
        T: IntoFuture + 'static,
        T::Future: Send + 'static
    {
        Then::new(self, f)
    }

    fn with_upgrade<U>(self, upgrade: U) -> Upgrade<Self, U>
    where
        Self: Sized,
        Self::Outbound: Send,
        Self::Output: AsyncRead + AsyncWrite + Send,
        U: OutboundUpgrade<Self::Output> + Clone + Send + 'static,
        U::NamesIter: Send + Clone,
        U::UpgradeId: Send,
        U::Future: Send
    {
        Upgrade::new(self, upgrade)
    }
}

impl<D: Dialer> DialerExt for D {}

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

impl<D, L> Transport<D, L>
where
    L: Listener
{
    pub fn or_listener<T>(self, other: T) -> Transport<D, Or<L, T>> {
        Transport {
            dialer: self.dialer,
            listener: Or::new(self.listener, other)
        }
    }

    pub fn map_listener_output<F, T>(self, f: F) -> Transport<D, Map<L, F>>
    where
        F: FnOnce(L::Output, Multiaddr) -> T + Clone + 'static
    {
        Transport {
            dialer: self.dialer,
            listener: Map::new(self.listener, f)
        }
    }

    pub fn map_listener_error<F, E>(self, f: F) -> Transport<D, MapErr<L, F>>
    where
        F: FnOnce(L::Error, Multiaddr) -> E + Clone + 'static
    {
        Transport {
            dialer: self.dialer,
            listener: MapErr::new(self.listener, f)
        }
    }

    pub fn listener_and_then<F, T>(self, f: F) -> Transport<D, AndThen<L, F>>
    where
        F: FnOnce(L::Output, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Error = L::Error> + 'static,
        T::Future: Send + 'static
    {
        Transport {
            dialer: self.dialer,
            listener: AndThen::new(self.listener, f)
        }
    }

    pub fn listener_or_else<F, T>(self, f: F) -> Transport<D, OrElse<L, F>>
    where
        F: FnOnce(L::Error, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Item = L::Output> + 'static,
        T::Future: Send + 'static
    {
        Transport {
            dialer: self.dialer,
            listener: OrElse::new(self.listener, f)
        }
    }

    pub fn listener_then<F, T>(self, f: F) -> Transport<D, Then<L, F>>
    where
        F: FnOnce(Result<(L::Output, Multiaddr), L::Error>) -> T + Clone + 'static,
        T: IntoFuture + 'static,
        T::Future: Send + 'static
    {
        Transport {
            dialer: self.dialer,
            listener: Then::new(self.listener, f)
        }
    }

    pub fn with_listener_upgrade<U>(self, upgrade: U) -> Transport<D, Upgrade<L, U>>
    where
        L::Inbound: Send,
        L::Upgrade: Send,
        L::Output: AsyncRead + AsyncWrite + Send,
        U: InboundUpgrade<L::Output> + Clone + Send + 'static,
        U::NamesIter: Send + Clone,
        U::UpgradeId: Send,
        U::Future: Send
    {
        Transport {
            dialer: self.dialer,
            listener: Upgrade::new(self.listener, upgrade)
        }
    }
}

impl<D, L> Transport<D, L>
where
    D: Dialer
{
    pub fn or_dialer<T>(self, other: T) -> Transport<Or<D, T>, L> {
        Transport {
            dialer: Or::new(self.dialer, other),
            listener: self.listener
        }
    }

    pub fn map_dialer_output<F, T>(self, f: F) -> Transport<Map<D, F>, L>
    where
        F: FnOnce(D::Output, Multiaddr) -> T + Clone + 'static
    {
        Transport {
            dialer: Map::new(self.dialer, f),
            listener: self.listener
        }
    }

    pub fn map_dialer_error<F, E>(self, f: F) -> Transport<MapErr<D, F>, L>
    where
        F: FnOnce(D::Error, Multiaddr) -> E + Clone + 'static
    {
        Transport {
            dialer: MapErr::new(self.dialer, f),
            listener: self.listener
        }
    }

    pub fn dialer_and_then<F, T>(self, f: F) -> Transport<AndThen<D, F>, L>
    where
        F: FnOnce(D::Output, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Error = D::Error> + 'static,
        T::Future: Send + 'static
    {
        Transport {
            dialer: AndThen::new(self.dialer, f),
            listener: self.listener
        }
    }

    pub fn dialer_or_else<F, T>(self, f: F) -> Transport<OrElse<D, F>, L>
    where
        F: FnOnce(D::Error, Multiaddr) -> T + Clone + 'static,
        T: IntoFuture<Item = D::Output> + 'static,
        T::Future: Send + 'static
    {
        Transport {
            dialer: OrElse::new(self.dialer, f),
            listener: self.listener
        }
    }

    pub fn dialer_then<F, T>(self, f: F) -> Transport<Then<D, F>, L>
    where
        F: FnOnce(Result<(D::Output, Multiaddr), D::Error>) -> T + Clone + 'static,
        T: IntoFuture + 'static,
        T::Future: Send + 'static
    {
        Transport {
            dialer: Then::new(self.dialer, f),
            listener: self.listener
        }
    }

    pub fn with_dialer_upgrade<U>(self, upgrade: U) -> Transport<Upgrade<D, U>, L>
    where
        D::Outbound: Send,
        D::Output: AsyncRead + AsyncWrite + Send,
        U: OutboundUpgrade<D::Output> + Clone + Send + 'static,
        U::NamesIter: Send + Clone,
        U::UpgradeId: Send,
        U::Future: Send
    {
        Transport {
            dialer: Upgrade::new(self.dialer, upgrade),
            listener: self.listener
        }
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
