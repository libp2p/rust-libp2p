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

pub mod apply;
pub mod denied;
pub mod error;

use bytes::Bytes;
use futures::future::Future;

pub use self::error::Error;
pub use self::denied::DeniedUpgrade;
pub use self::apply::{apply_inbound, apply_outbound};

pub trait UpgradeInfo {
    type UpgradeId;
    type NamesIter: Iterator<Item = (Bytes, Self::UpgradeId)>;

    fn protocol_names(&self) -> Self::NamesIter;
}

pub trait InboundUpgrade<C>: UpgradeInfo {
    type Output;
    type Error;
    type Future: Future<Item = Self::Output, Error = Self::Error>;

    fn upgrade_inbound(self, socket: C, id: Self::UpgradeId) -> Self::Future;
}

pub trait OutboundUpgrade<C>: UpgradeInfo {
    type Output;
    type Error;
    type Future: Future<Item = Self::Output, Error = Self::Error>;

    fn upgrade_outbound(self, socket: C, id: Self::UpgradeId) -> Self::Future;
}

