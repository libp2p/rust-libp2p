// Copyright 2020 Parity Technologies (UK) Ltd.
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
mod either;
mod pending;
mod ready;
mod select;

use crate::Stream;
use futures::prelude::*;

pub use denied::DeniedUpgrade;
pub use pending::PendingUpgrade;
pub use ready::ReadyUpgrade;
pub use select::SelectUpgrade;

/// Common trait for upgrades that can be applied on inbound substreams, outbound substreams,
/// or both.
pub trait UpgradeInfo: Send + 'static {
    /// Opaque type representing a negotiable protocol.
    type Info: AsRef<str> + Clone + Send + 'static;
    /// Iterator returned by `protocol_info`.
    type InfoIter: IntoIteratorSend<Item = Self::Info> + Send + 'static;

    /// Returns the list of protocols that are supported. Used during the negotiation process.
    fn protocol_info(&self) -> Self::InfoIter;
}

/// Possible upgrade on an inbound connection or substream.
pub trait InboundUpgrade: UpgradeInfo {
    /// Output after the upgrade has been successfully negotiated and the handshake performed.
    type Output: Send + 'static;
    /// Possible error during the handshake.
    type Error: Send + 'static;
    /// Future that performs the handshake with the remote.
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start the handshake.
    ///
    /// The `info` is the identifier of the protocol, as produced by `protocol_info`.
    fn upgrade_inbound(self, socket: Stream, info: Self::Info) -> Self::Future;
}

/// Possible upgrade on an outbound connection or substream.
pub trait OutboundUpgrade: UpgradeInfo {
    /// Output after the upgrade has been successfully negotiated and the handshake performed.
    type Output: Send + 'static;
    /// Possible error during the handshake.
    type Error: Send + 'static;
    /// Future that performs the handshake with the remote.
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    /// After we have determined that the remote supports one of the protocols we support, this
    /// method is called to start the handshake.
    ///
    /// The `info` is the identifier of the protocol, as produced by `protocol_info`.
    fn upgrade_outbound(self, socket: Stream, info: Self::Info) -> Self::Future;
}

pub trait IntoIteratorSend {
    type Item: Send + 'static;
    type IntoIter: Iterator<Item = Self::Item> + Send + 'static;

    fn into_iter_send(self) -> Self::IntoIter;
}

impl<I> IntoIteratorSend for I
where
    I: IntoIterator,
    I::Item: Send + 'static,
    I::IntoIter: Send + 'static,
{
    type Item = I::Item;
    type IntoIter = I::IntoIter;

    fn into_iter_send(self) -> <Self as IntoIteratorSend>::IntoIter {
        <Self as IntoIterator>::into_iter(self)
    }
}
