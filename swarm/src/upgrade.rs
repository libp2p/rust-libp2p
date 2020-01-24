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

use crate::NegotiatedBoxSubstream;

use futures::prelude::*;
use libp2p_core::{either, upgrade};

pub trait UpgradeInfoSend: Send + 'static {
    type Info: upgrade::ProtocolName + Clone + Send + 'static;
    type InfoIter: Iterator<Item = Self::Info> + Send + 'static;

    fn protocol_info(&self) -> Self::InfoIter;
}

impl<T> UpgradeInfoSend for T
where
    T: upgrade::UpgradeInfo + Send + 'static,
    T::Info: Send + 'static,
    <T::InfoIter as IntoIterator>::IntoIter: Send + 'static,
{
    type Info = T::Info;
    type InfoIter = <T::InfoIter as IntoIterator>::IntoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        upgrade::UpgradeInfo::protocol_info(self).into_iter()
    }
}

pub trait OutboundUpgradeSend: UpgradeInfoSend {
    type Output: Send + 'static;
    type Error: Send + 'static;
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    fn upgrade_outbound(self, socket: NegotiatedBoxSubstream, info: Self::Info) -> Self::Future;
}

impl<T, TInfo> OutboundUpgradeSend for T
where
    T: upgrade::OutboundUpgrade<NegotiatedBoxSubstream, Info = TInfo> + UpgradeInfoSend<Info = TInfo>,
    TInfo: upgrade::ProtocolName + Clone + Send + 'static,
    T::Output: Send + 'static,
    T::Error: Send + 'static,
    T::Future: Send + 'static,
{
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_outbound(self, socket: NegotiatedBoxSubstream, info: TInfo) -> Self::Future {
        upgrade::OutboundUpgrade::upgrade_outbound(self, socket, info)
    }
}

pub trait InboundUpgradeSend: UpgradeInfoSend {
    type Output: Send + 'static;
    type Error: Send + 'static;
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    fn upgrade_inbound(self, socket: NegotiatedBoxSubstream, info: Self::Info) -> Self::Future;
}

impl<T, TInfo> InboundUpgradeSend for T
where
    T: upgrade::InboundUpgrade<NegotiatedBoxSubstream, Info = TInfo> + UpgradeInfoSend<Info = TInfo>,
    TInfo: upgrade::ProtocolName + Clone + Send + 'static,
    T::Output: Send + 'static,
    T::Error: Send + 'static,
    T::Future: Send + 'static,
{
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_inbound(self, socket: NegotiatedBoxSubstream, info: TInfo) -> Self::Future {
        upgrade::InboundUpgrade::upgrade_inbound(self, socket, info)
    }
}

pub struct SendWrapper<T>(pub T);

impl<T: UpgradeInfoSend> upgrade::UpgradeInfo for SendWrapper<T> {
    type Info = T::Info;
    type InfoIter = T::InfoIter;

    fn protocol_info(&self) -> Self::InfoIter {
        UpgradeInfoSend::protocol_info(self)
    }
}

impl<T: OutboundUpgradeSend> upgrade::OutboundUpgrade<NegotiatedBoxSubstream> for SendWrapper<T> {
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_outbound(self, socket: NegotiatedBoxSubstream, info: T::Info) -> Self::Future {
        OutboundUpgradeSend::upgrade_outbound(self, socket, info)
    }
}

impl<T: InboundUpgradeSend> upgrade::InboundUpgrade<NegotiatedBoxSubstream> for SendWrapper<T> {
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_inbound(self, socket: NegotiatedBoxSubstream, info: T::Info) -> Self::Future {
        InboundUpgradeSend::upgrade_inbound(self, socket, info)
    }
}
