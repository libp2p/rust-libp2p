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

use futures::future::{self, TryFutureExt};
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};

/// Upgrade applying a function to an inner upgrade.
#[derive(Debug, Clone)]
pub(crate) struct Upgrade<U, F> {
    upgrade: U,
    fun: F,
}

impl<U, F> Upgrade<U, F> {
    /// Applies the function on the result of the upgrade.
    pub(crate) fn new(upgrade: U, fun: F) -> Self {
        Upgrade { upgrade, fun }
    }
}

impl<U, F> UpgradeInfo for Upgrade<U, F>
where
    U: UpgradeInfo,
{
    type Info = U::Info;
    type InfoIter = U::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrade.protocol_info()
    }
}

impl<U, F, C, D> InboundUpgrade<C> for Upgrade<U, F>
where
    U: InboundUpgrade<C>,
    F: FnOnce(U::Output) -> D,
{
    type Output = D;
    type Error = U::Error;
    type Future = future::MapOk<U::Future, F>;

    fn upgrade_inbound(self, sock: C, info: Self::Info) -> Self::Future {
        self.upgrade.upgrade_inbound(sock, info).map_ok(self.fun)
    }
}

impl<U, F, C, D> OutboundUpgrade<C> for Upgrade<U, F>
where
    U: OutboundUpgrade<C>,
    F: FnOnce(U::Output) -> D,
{
    type Output = D;
    type Error = U::Error;
    type Future = future::MapOk<U::Future, F>;

    fn upgrade_outbound(self, sock: C, info: Self::Info) -> Self::Future {
        self.upgrade.upgrade_outbound(sock, info).map_ok(self.fun)
    }
}
