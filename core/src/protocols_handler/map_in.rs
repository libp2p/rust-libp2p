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
    protocols_handler::{ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr},
    upgrade::{
        InboundUpgrade,
        OutboundUpgrade,
    }
};
use futures::prelude::*;
use std::{io, marker::PhantomData};

/// Wrapper around a protocol handler that turns the input event into something else.
pub struct MapInEvent<TProtoHandler, TNewIn, TMap> {
    inner: TProtoHandler,
    map: TMap,
    marker: PhantomData<TNewIn>,
}

impl<TProtoHandler, TMap, TNewIn> MapInEvent<TProtoHandler, TNewIn, TMap> {
    /// Creates a `MapInEvent`.
    #[inline]
    pub(crate) fn new(inner: TProtoHandler, map: TMap) -> Self {
        MapInEvent {
            inner,
            map,
            marker: PhantomData,
        }
    }
}

impl<TProtoHandler, TMap, TNewIn> ProtocolsHandler for MapInEvent<TProtoHandler, TNewIn, TMap>
where
    TProtoHandler: ProtocolsHandler,
    TMap: Fn(TNewIn) -> Option<TProtoHandler::InEvent>,
{
    type InEvent = TNewIn;
    type OutEvent = TProtoHandler::OutEvent;
    type Substream = TProtoHandler::Substream;
    type InboundProtocol = TProtoHandler::InboundProtocol;
    type OutboundProtocol = TProtoHandler::OutboundProtocol;
    type OutboundOpenInfo = TProtoHandler::OutboundOpenInfo;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        self.inner.listen_protocol()
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output
    ) {
        self.inner.inject_fully_negotiated_inbound(protocol)
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        info: Self::OutboundOpenInfo
    ) {
        self.inner.inject_fully_negotiated_outbound(protocol, info)
    }

    #[inline]
    fn inject_event(&mut self, event: TNewIn) {
        if let Some(event) = (self.map)(event) {
            self.inner.inject_event(event);
        }
    }

    #[inline]
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {
        self.inner.inject_dial_upgrade_error(info, error)
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {
        self.inner.inject_inbound_closed()
    }

    #[inline]
    fn shutdown(&mut self) {
        self.inner.shutdown()
    }

    #[inline]
    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>,
        io::Error,
    > {
        self.inner.poll()
    }
}
