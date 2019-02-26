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

use futures::future::{self, FutureResult};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use std::iter;
use void::Void;

#[derive(Debug, Copy, Clone)]
pub struct PlainTextConfig;

impl UpgradeInfo for PlainTextConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/plaintext/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for PlainTextConfig {
    type Output = C;
    type Error = Void;
    type Future = FutureResult<C, Self::Error>;

    fn upgrade_inbound(self, i: C, _: Self::Info) -> Self::Future {
        future::ok(i)
    }
}

impl<C> OutboundUpgrade<C> for PlainTextConfig {
    type Output = C;
    type Error = Void;
    type Future = FutureResult<C, Self::Error>;

    fn upgrade_outbound(self, i: C, _: Self::Info) -> Self::Future {
        future::ok(i)
    }
}

