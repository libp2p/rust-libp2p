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

use crate::NegotiatedSubstream;
use crate::protocols_handler::{
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr
};
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, DeniedUpgrade};
use std::task::{Context, Poll};
use void::Void;

/// Implementation of `ProtocolsHandler` that doesn't handle anything.
pub struct DummyProtocolsHandler {
}

impl Default for DummyProtocolsHandler {
    fn default() -> Self {
        DummyProtocolsHandler {
        }
    }
}

impl ProtocolsHandler for DummyProtocolsHandler {
    type InEvent = Void;
    type OutEvent = Void;
    type Error = Void;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Void;

    #[inline]
    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade)
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        _: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output
    ) {
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        _: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::OutboundOpenInfo
    ) {
    }

    #[inline]
    fn inject_event(&mut self, _: Self::InEvent) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error>) {}

    #[inline]
    fn connection_keep_alive(&self) -> KeepAlive { KeepAlive::No }

    #[inline]
    fn poll(
        &mut self,
        _: &mut Context,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>,
    > {
        Poll::Pending
    }
}
