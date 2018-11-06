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
use crate::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};

#[derive(Debug, Clone)]
pub struct MapUpgrade<U, F> { upgrade: U, fun: F }

impl<U, F> MapUpgrade<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapUpgrade { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for MapUpgrade<U, F>
where
    U: UpgradeInfo
{
    type UpgradeId = U::UpgradeId;
    type NamesIter = U::NamesIter;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }
}

impl<C, U, F, T> InboundUpgrade<C> for MapUpgrade<U, F>
where
    U: InboundUpgrade<C> + Send,
    U::Future: Send + 'static,
    F: FnOnce(U::Output) -> T + Clone + Send + 'static
{
    type Output = T;
    type Error = U::Error;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_inbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        let fun = self.fun;
        Box::new(self.upgrade.upgrade_inbound(sock, id).map(fun))
    }
}

impl<C, U, F, T> OutboundUpgrade<C> for MapUpgrade<U, F>
where
    U: OutboundUpgrade<C> + Send,
    U::Future: Send + 'static,
    F: FnOnce(U::Output) -> T + Clone + Send + 'static
{
    type Output = T;
    type Error = U::Error;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_outbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        let fun = self.fun;
        Box::new(self.upgrade.upgrade_outbound(sock, id).map(fun))
    }
}

#[derive(Debug, Clone)]
pub struct MapUpgradeErr<U, F> { upgrade: U, fun: F }

impl<U, F> MapUpgradeErr<U, F> {
    pub fn new(upgrade: U, fun: F) -> Self {
        MapUpgradeErr { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for MapUpgradeErr<U, F>
where
    U: UpgradeInfo
{
    type UpgradeId = U::UpgradeId;
    type NamesIter = U::NamesIter;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }
}

impl<C, U, F, T> InboundUpgrade<C> for MapUpgradeErr<U, F>
where
    U: InboundUpgrade<C> + Send,
    U::Future: Send + 'static,
    F: FnOnce(U::Error) -> T + Clone + Send + 'static
{
    type Output = U::Output;
    type Error = T;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_inbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        let fun = self.fun;
        Box::new(self.upgrade.upgrade_inbound(sock, id).map_err(fun))
    }
}

impl<C, U, F, T> OutboundUpgrade<C> for MapUpgradeErr<U, F>
where
    U: OutboundUpgrade<C> + Send,
    U::Future: Send + 'static,
    F: FnOnce(U::Error) -> T + Clone + Send + 'static
{
    type Output = U::Output;
    type Error = T;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_outbound(self, sock: C, id: Self::UpgradeId) -> Self::Future {
        let fun = self.fun;
        Box::new(self.upgrade.upgrade_outbound(sock, id).map_err(fun))
    }
}

