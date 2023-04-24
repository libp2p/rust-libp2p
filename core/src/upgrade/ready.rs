// Copyright 2022 Protocol Labs.
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

use crate::upgrade::Protocol;
use crate::upgrade::{InboundUpgrade, OutboundUpgrade};
use crate::ToProtocolsIter;
use futures::future;
use std::iter;
use void::Void;

/// Implementation of [`UpgradeInfo`], [`InboundUpgrade`] and [`OutboundUpgrade`] that directly yields the substream.
#[derive(Debug, Clone)]
pub struct ReadyUpgrade {
    protocol_name: Protocol,
}

impl ReadyUpgrade {
    pub fn new(protocol_name: Protocol) -> Self {
        Self { protocol_name }
    }
}

impl ToProtocolsIter for ReadyUpgrade {
    type Iter = iter::Once<Protocol>;

    fn to_protocols_iter(&self) -> Self::Iter {
        iter::once(self.protocol_name.clone())
    }
}

impl<C> InboundUpgrade<C> for ReadyUpgrade {
    type Output = C;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, stream: C, _: Protocol) -> Self::Future {
        future::ready(Ok(stream))
    }
}

impl<C> OutboundUpgrade<C> for ReadyUpgrade {
    type Output = C;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, stream: C, _: Protocol) -> Self::Future {
        future::ready(Ok(stream))
    }
}
