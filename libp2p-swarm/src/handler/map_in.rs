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

use crate::handler::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    SubstreamProtocol,
};
use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend};
use libp2p_core::Multiaddr;
use std::{fmt::Debug, marker::PhantomData, task::Context, task::Poll};

/// Wrapper around a protocol handler that turns the input event into something else.
pub struct MapInEvent<TConnectionHandler, TNewIn, TMap> {
    inner: TConnectionHandler,
    map: TMap,
    marker: PhantomData<TNewIn>,
}

impl<TConnectionHandler, TMap, TNewIn> MapInEvent<TConnectionHandler, TNewIn, TMap> {
    /// Creates a `MapInEvent`.
    pub(crate) fn new(inner: TConnectionHandler, map: TMap) -> Self {
        MapInEvent {
            inner,
            map,
            marker: PhantomData,
        }
    }
}

impl<TConnectionHandler, TMap, TNewIn> ConnectionHandler
    for MapInEvent<TConnectionHandler, TNewIn, TMap>
where
    TConnectionHandler: ConnectionHandler,
    TMap: Fn(TNewIn) -> Option<TConnectionHandler::InEvent>,
    TNewIn: Debug + Send + 'static,
    TMap: Send + 'static,
{
    type InEvent = TNewIn;
    type OutEvent = TConnectionHandler::OutEvent;
    type Error = TConnectionHandler::Error;
    type InboundProtocol = TConnectionHandler::InboundProtocol;
    type OutboundProtocol = TConnectionHandler::OutboundProtocol;
    type InboundOpenInfo = TConnectionHandler::InboundOpenInfo;
    type OutboundOpenInfo = TConnectionHandler::OutboundOpenInfo;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        self.inner.listen_protocol()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        info: Self::InboundOpenInfo,
    ) {
        self.inner.inject_fully_negotiated_inbound(protocol, info)
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        self.inner.inject_fully_negotiated_outbound(protocol, info)
    }

    fn inject_event(&mut self, event: TNewIn) {
        if let Some(event) = (self.map)(event) {
            self.inner.inject_event(event);
        }
    }

    fn inject_address_change(&mut self, addr: &Multiaddr) {
        self.inner.inject_address_change(addr)
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ConnectionHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        self.inner.inject_dial_upgrade_error(info, error)
    }

    fn inject_listen_upgrade_error(
        &mut self,
        info: Self::InboundOpenInfo,
        error: ConnectionHandlerUpgrErr<<Self::InboundProtocol as InboundUpgradeSend>::Error>,
    ) {
        self.inner.inject_listen_upgrade_error(info, error)
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.inner.connection_keep_alive()
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        self.inner.poll(cx)
    }
}
