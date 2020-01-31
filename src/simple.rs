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

use crate::core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use bytes::Bytes;
use futures::prelude::*;
use std::{iter, sync::Arc};

/// Implementation of `ConnectionUpgrade`. Convenient to use with small protocols.
#[derive(Debug)]
pub struct SimpleProtocol<F> {
    info: Bytes,
    // Note: we put the closure `F` in an `Arc` because Rust closures aren't automatically clonable
    // yet.
    upgrade: Arc<F>,
}

impl<F> SimpleProtocol<F> {
    /// Builds a `SimpleProtocol`.
    pub fn new<N>(info: N, upgrade: F) -> SimpleProtocol<F>
    where
        N: Into<Bytes>,
    {
        SimpleProtocol {
            info: info.into(),
            upgrade: Arc::new(upgrade),
        }
    }
}

impl<F> Clone for SimpleProtocol<F> {
    fn clone(&self) -> Self {
        SimpleProtocol {
            info: self.info.clone(),
            upgrade: self.upgrade.clone(),
        }
    }
}

impl<F> UpgradeInfo for SimpleProtocol<F> {
    type Info = Bytes;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.info.clone())
    }
}

impl<C, F, O, A, E> InboundUpgrade<C> for SimpleProtocol<F>
where
    C: AsyncRead + AsyncWrite,
    F: Fn(C) -> O,
    O: Future<Output = Result<A, E>>,
{
    type Output = A;
    type Error = E;
    type Future = O;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        let upgrade = &self.upgrade;
        upgrade(socket)
    }
}

impl<C, F, O, A, E> OutboundUpgrade<C> for SimpleProtocol<F>
where
    C: AsyncRead + AsyncWrite,
    F: Fn(C) -> O,
    O: Future<Output = Result<A, E>>,
{
    type Output = A;
    type Error = E;
    type Future = O;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        let upgrade = &self.upgrade;
        upgrade(socket)
    }
}
