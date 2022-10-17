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

use crate::NegotiatedSubstream;

use futures::prelude::*;
use libp2p_core::upgrade;
use std::iter;

/// Implemented automatically on all types that implement [`UpgradeInfo`](upgrade::UpgradeInfo)
/// and `Send + 'static`.
///
/// Do not implement this trait yourself. Instead, please implement
/// [`UpgradeInfo`](upgrade::UpgradeInfo).
pub trait UpgradeInfoSend: Send + 'static {
/// Equivalent to [`UpgradeInfo::Info`](upgrade::UpgradeInfo::Info).
//type Info: upgrade::ProtocolName + Clone + Send + 'static;
    /// Equivalent to [`UpgradeInfo::InfoIter`](upgrade::UpgradeInfo::InfoIter).
    type InfoIter: Iterator<Item = upgrade::ProtocolName> + Send + 'static;

    /// Equivalent to [`UpgradeInfo::protocol_info`](upgrade::UpgradeInfo::protocol_info).
    fn protocol_info(&self) -> Self::InfoIter;
}

impl UpgradeInfoSend for upgrade::ProtocolName {
    type InfoIter = iter::Once<upgrade::ProtocolName>;

    fn protocol_info(&self) -> Self::InfoIter {
        upgrade::UpgradeInfo::protocol_info(self).into_iter()
    }
}

/// Implemented automatically on all types that implement
/// [`OutboundUpgrade`](upgrade::OutboundUpgrade) and `Send + 'static`.
///
/// Do not implement this trait yourself. Instead, please implement
/// [`OutboundUpgrade`](upgrade::OutboundUpgrade).
pub trait OutboundUpgradeSend: UpgradeInfoSend {
    /// Equivalent to [`OutboundUpgrade::Output`](upgrade::OutboundUpgrade::Output).
    type Output: Send + 'static;
    /// Equivalent to [`OutboundUpgrade::Error`](upgrade::OutboundUpgrade::Error).
    type Error: Send + 'static;
    /// Equivalent to [`OutboundUpgrade::Future`](upgrade::OutboundUpgrade::Future).
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    /// Equivalent to [`OutboundUpgrade::upgrade_outbound`](upgrade::OutboundUpgrade::upgrade_outbound).
    fn upgrade_outbound(self, socket: NegotiatedSubstream, info: upgrade::ProtocolName) -> Self::Future;
}

impl<T> OutboundUpgradeSend for T
where
    T: upgrade::OutboundUpgrade<NegotiatedSubstream> + UpgradeInfoSend,
    T::Output: Send + 'static,
    T::Error: Send + 'static,
    T::Future: Send + 'static,
{
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_outbound(self, socket: NegotiatedSubstream, info: upgrade::ProtocolName) -> Self::Future {
        upgrade::OutboundUpgrade::upgrade_outbound(self, socket, info)
    }
}

/// Implemented automatically on all types that implement
/// [`InboundUpgrade`](upgrade::InboundUpgrade) and `Send + 'static`.
///
/// Do not implement this trait yourself. Instead, please implement
/// [`InboundUpgrade`](upgrade::InboundUpgrade).
pub trait InboundUpgradeSend: UpgradeInfoSend {
    /// Equivalent to [`InboundUpgrade::Output`](upgrade::InboundUpgrade::Output).
    type Output: Send + 'static;
    /// Equivalent to [`InboundUpgrade::Error`](upgrade::InboundUpgrade::Error).
    type Error: Send + 'static;
    /// Equivalent to [`InboundUpgrade::Future`](upgrade::InboundUpgrade::Future).
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    /// Equivalent to [`InboundUpgrade::upgrade_inbound`](upgrade::InboundUpgrade::upgrade_inbound).
    fn upgrade_inbound(self, socket: NegotiatedSubstream, info: upgrade::ProtocolName) -> Self::Future;
}

impl<T> InboundUpgradeSend for T
where
    T: upgrade::InboundUpgrade<NegotiatedSubstream> + UpgradeInfoSend,
    T::Output: Send + 'static,
    T::Error: Send + 'static,
    T::Future: Send + 'static,
{
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_inbound(self, socket: NegotiatedSubstream, info: upgrade::ProtocolName) -> Self::Future {
        upgrade::InboundUpgrade::upgrade_inbound(self, socket, info)
    }
}

/// Wraps around a type that implements [`OutboundUpgradeSend`], [`InboundUpgradeSend`], or
/// both, and implements [`OutboundUpgrade`](upgrade::OutboundUpgrade) and/or
/// [`InboundUpgrade`](upgrade::InboundUpgrade).
///
/// > **Note**: This struct is mostly an implementation detail of the library and normally
/// >           doesn't need to be used directly.
pub struct SendWrapper<T>(pub T);

impl<T: UpgradeInfoSend> upgrade::UpgradeInfo for SendWrapper<T> {
    type InfoIter = T::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        UpgradeInfoSend::protocol_info(&self.0)
    }
}

impl<T: OutboundUpgradeSend> upgrade::OutboundUpgrade<NegotiatedSubstream> for SendWrapper<T> {
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_outbound(self, socket: NegotiatedSubstream, info: upgrade::ProtocolName) -> Self::Future {
        OutboundUpgradeSend::upgrade_outbound(self.0, socket, info)
    }
}

impl<T: InboundUpgradeSend> upgrade::InboundUpgrade<NegotiatedSubstream> for SendWrapper<T> {
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_inbound(self, socket: NegotiatedSubstream, info: upgrade::ProtocolName) -> Self::Future {
        InboundUpgradeSend::upgrade_inbound(self.0, socket, info)
    }
}
