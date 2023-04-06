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

use crate::{NegotiatedSubstream, Protocol};

use futures::prelude::*;
use libp2p_core::upgrade;
use libp2p_core::upgrade::UpgradeProtocols;

/// Implemented automatically on all types that implement
/// [`OutboundUpgrade`](upgrade::OutboundUpgrade) and `Send + 'static`.
///
/// Do not implement this trait yourself. Instead, please implement
/// [`OutboundUpgrade`](upgrade::OutboundUpgrade).
pub trait OutboundUpgradeSend: UpgradeProtocols + Send {
    /// Equivalent to [`OutboundUpgrade::Output`](upgrade::OutboundUpgrade::Output).
    type Output: Send + 'static;
    /// Equivalent to [`OutboundUpgrade::Error`](upgrade::OutboundUpgrade::Error).
    type Error: Send + 'static;
    /// Equivalent to [`OutboundUpgrade::Future`](upgrade::OutboundUpgrade::Future).
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    /// Equivalent to [`OutboundUpgrade::upgrade_outbound`](upgrade::OutboundUpgrade::upgrade_outbound).
    fn upgrade_outbound(
        self,
        socket: NegotiatedSubstream,
        selected_protocol: Protocol,
    ) -> Self::Future;
}

impl<T> OutboundUpgradeSend for T
where
    T: upgrade::OutboundUpgrade<NegotiatedSubstream> + Send,
    T::Output: Send + 'static,
    T::Error: Send + 'static,
    T::Future: Send + 'static,
{
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_outbound(
        self,
        socket: NegotiatedSubstream,
        selected_protocol: Protocol,
    ) -> Self::Future {
        upgrade::OutboundUpgrade::upgrade_outbound(self, socket, selected_protocol)
    }
}

/// Implemented automatically on all types that implement
/// [`InboundUpgrade`](upgrade::InboundUpgrade) and `Send + 'static`.
///
/// Do not implement this trait yourself. Instead, please implement
/// [`InboundUpgrade`](upgrade::InboundUpgrade).
pub trait InboundUpgradeSend: UpgradeProtocols + Send {
    /// Equivalent to [`InboundUpgrade::Output`](upgrade::InboundUpgrade::Output).
    type Output: Send + 'static;
    /// Equivalent to [`InboundUpgrade::Error`](upgrade::InboundUpgrade::Error).
    type Error: Send + 'static;
    /// Equivalent to [`InboundUpgrade::Future`](upgrade::InboundUpgrade::Future).
    type Future: Future<Output = Result<Self::Output, Self::Error>> + Send + 'static;

    /// Equivalent to [`InboundUpgrade::upgrade_inbound`](upgrade::InboundUpgrade::upgrade_inbound).
    fn upgrade_inbound(
        self,
        socket: NegotiatedSubstream,
        selected_protocol: Protocol,
    ) -> Self::Future;
}

impl<T> InboundUpgradeSend for T
where
    T: upgrade::InboundUpgrade<NegotiatedSubstream> + Send,
    T::Output: Send + 'static,
    T::Error: Send + 'static,
    T::Future: Send + 'static,
{
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_inbound(
        self,
        socket: NegotiatedSubstream,
        selected_protocol: Protocol,
    ) -> Self::Future {
        upgrade::InboundUpgrade::upgrade_inbound(self, socket, selected_protocol)
    }
}

/// Wraps around a type that implements [`OutboundUpgradeSend`], [`InboundUpgradeSend`], or
/// both, and implements [`OutboundUpgrade`](upgrade::OutboundUpgrade) and/or
/// [`InboundUpgrade`](upgrade::InboundUpgrade).
///
/// > **Note**: This struct is mostly an implementation detail of the library and normally
/// >           doesn't need to be used directly.
pub struct SendWrapper<T>(pub T);

impl<T: upgrade::UpgradeProtocols> upgrade::UpgradeProtocols for SendWrapper<T> {
    fn protocols(&self) -> Vec<Protocol> {
        self.0.protocols()
    }
}

impl<T: OutboundUpgradeSend> upgrade::OutboundUpgrade<NegotiatedSubstream> for SendWrapper<T> {
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_outbound(self, socket: NegotiatedSubstream, info: Protocol) -> Self::Future {
        OutboundUpgradeSend::upgrade_outbound(self.0, socket, info)
    }
}

impl<T: InboundUpgradeSend> upgrade::InboundUpgrade<NegotiatedSubstream> for SendWrapper<T> {
    type Output = T::Output;
    type Error = T::Error;
    type Future = T::Future;

    fn upgrade_inbound(self, socket: NegotiatedSubstream, info: Protocol) -> Self::Future {
        InboundUpgradeSend::upgrade_inbound(self.0, socket, info)
    }
}
