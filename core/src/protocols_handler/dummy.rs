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

use crate::{
    protocols_handler::{KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr},
    upgrade::{
        InboundUpgrade,
        OutboundUpgrade,
        DeniedUpgrade,
    }
};
use futures::prelude::*;
use std::marker::PhantomData;
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;

/// Implementation of `ProtocolsHandler` that doesn't handle anything.
pub struct DummyProtocolsHandler<TSubstream> {
    shutting_down: bool,
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Default for DummyProtocolsHandler<TSubstream> {
    #[inline]
    fn default() -> Self {
        DummyProtocolsHandler {
            shutting_down: false,
            marker: PhantomData,
        }
    }
}

impl<TSubstream> ProtocolsHandler for DummyProtocolsHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = Void;
    type OutEvent = Void;
    type Error = Void;
    type Substream = TSubstream;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Void;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        DeniedUpgrade
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        _: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output
    ) {
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        _: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        _: Self::OutboundOpenInfo
    ) {
    }

    #[inline]
    fn inject_event(&mut self, _: Self::InEvent) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {}

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn connection_keep_alive(&self) -> KeepAlive { KeepAlive::Now }

    #[inline]
    fn shutdown(&mut self) {
        self.shutting_down = true;
    }

    #[inline]
    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        Void,
    > {
        if self.shutting_down {
            Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown))
        } else {
            Ok(Async::NotReady)
        }
    }
}
