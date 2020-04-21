// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::{NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters};
use crate::upgrade::{SendWrapper, InboundUpgradeSend, OutboundUpgradeSend};
use crate::protocols_handler::{
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr,
    IntoProtocolsHandler
};

use libp2p_core::{
    ConnectedPoint,
    PeerId,
    Multiaddr,
    connection::ConnectionId,
    either::EitherOutput,
    upgrade::{DeniedUpgrade, EitherUpgrade}
};
use std::{error, task::Context, task::Poll};

/// Implementation of `NetworkBehaviour` that can be either in the disabled or enabled state.
///
/// The state can only be chosen at initialization.
pub struct Toggle<TBehaviour> {
    inner: Option<TBehaviour>,
}

impl<TBehaviour> Toggle<TBehaviour> {
    /// Returns `true` if `Toggle` is enabled and `false` if it's disabled.
    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }
}

impl<TBehaviour> From<Option<TBehaviour>> for Toggle<TBehaviour> {
    fn from(inner: Option<TBehaviour>) -> Self {
        Toggle { inner }
    }
}

impl<TBehaviour> NetworkBehaviour for Toggle<TBehaviour>
where
    TBehaviour: NetworkBehaviour
{
    type ProtocolsHandler = ToggleIntoProtoHandler<TBehaviour::ProtocolsHandler>;
    type OutEvent = TBehaviour::OutEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        ToggleIntoProtoHandler {
            inner: self.inner.as_mut().map(|i| i.new_handler())
        }
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.inner.as_mut().map(|b| b.addresses_of_peer(peer_id)).unwrap_or_else(Vec::new)
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_connected(peer_id)
        }
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_disconnected(peer_id)
        }
    }

    fn inject_connection_established(&mut self, peer_id: &PeerId, connection: &ConnectionId, endpoint: &ConnectedPoint) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_connection_established(peer_id, connection, endpoint)
        }
    }

    fn inject_connection_closed(&mut self, peer_id: &PeerId, connection: &ConnectionId, endpoint: &ConnectedPoint) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_connection_closed(peer_id, connection, endpoint)
        }
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection: ConnectionId,
        event: <<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::OutEvent
    ) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_event(peer_id, connection, event);
        }
    }

    fn inject_addr_reach_failure(&mut self, peer_id: Option<&PeerId>, addr: &Multiaddr, error: &dyn error::Error) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_addr_reach_failure(peer_id, addr, error)
        }
    }

    fn inject_dial_failure(&mut self, peer_id: &PeerId) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_dial_failure(peer_id)
        }
    }

    fn inject_new_listen_addr(&mut self, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_new_listen_addr(addr)
        }
    }

    fn inject_expired_listen_addr(&mut self, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_expired_listen_addr(addr)
        }
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_new_external_addr(addr)
        }
    }

    fn poll(&mut self, cx: &mut Context, params: &mut impl PollParameters)
        -> Poll<NetworkBehaviourAction<<<Self::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent, Self::OutEvent>>
    {
        if let Some(inner) = self.inner.as_mut() {
            inner.poll(cx, params)
        } else {
            Poll::Pending
        }
    }
}

impl<TEvent, TBehaviour> NetworkBehaviourEventProcess<TEvent> for Toggle<TBehaviour>
where
    TBehaviour: NetworkBehaviourEventProcess<TEvent>
{
    fn inject_event(&mut self, event: TEvent) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_event(event);
        }
    }
}

/// Implementation of `IntoProtocolsHandler` that can be in the disabled state.
pub struct ToggleIntoProtoHandler<TInner> {
    inner: Option<TInner>,
}

impl<TInner> IntoProtocolsHandler for ToggleIntoProtoHandler<TInner>
where
    TInner: IntoProtocolsHandler
{
    type Handler = ToggleProtoHandler<TInner::Handler>;

    fn into_handler(self, remote_peer_id: &PeerId, connected_point: &ConnectedPoint) -> Self::Handler {
        ToggleProtoHandler {
            inner: self.inner.map(|h| h.into_handler(remote_peer_id, connected_point))
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ProtocolsHandler>::InboundProtocol {
        if let Some(inner) = self.inner.as_ref() {
            EitherUpgrade::A(SendWrapper(inner.inbound_protocol()))
        } else {
            EitherUpgrade::B(SendWrapper(DeniedUpgrade))
        }
    }
}

/// Implementation of `ProtocolsHandler` that can be in the disabled state.
pub struct ToggleProtoHandler<TInner> {
    inner: Option<TInner>,
}

impl<TInner> ProtocolsHandler for ToggleProtoHandler<TInner>
where
    TInner: ProtocolsHandler,
{
    type InEvent = TInner::InEvent;
    type OutEvent = TInner::OutEvent;
    type Error = TInner::Error;
    type InboundProtocol = EitherUpgrade<SendWrapper<TInner::InboundProtocol>, SendWrapper<DeniedUpgrade>>;
    type OutboundProtocol = TInner::OutboundProtocol;
    type OutboundOpenInfo = TInner::OutboundOpenInfo;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        if let Some(inner) = self.inner.as_ref() {
            inner.listen_protocol().map_upgrade(|u| EitherUpgrade::A(SendWrapper(u)))
        } else {
            SubstreamProtocol::new(EitherUpgrade::B(SendWrapper(DeniedUpgrade)))
        }
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        out: <Self::InboundProtocol as InboundUpgradeSend>::Output
    ) {
        let out = match out {
            EitherOutput::First(out) => out,
            EitherOutput::Second(v) => void::unreachable(v),
        };

        self.inner.as_mut().expect("Can't receive an inbound substream if disabled; QED")
            .inject_fully_negotiated_inbound(out)
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        out: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo
    ) {
        self.inner.as_mut().expect("Can't receive an outbound substream if disabled; QED")
            .inject_fully_negotiated_outbound(out, info)
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        self.inner.as_mut().expect("Can't receive events if disabled; QED")
            .inject_event(event)
    }

    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, err: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>) {
        self.inner.as_mut().expect("Can't receive an outbound substream if disabled; QED")
            .inject_dial_upgrade_error(info, err)
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.inner.as_ref().map(|h| h.connection_keep_alive())
            .unwrap_or(KeepAlive::No)
    }

    fn poll(
        &mut self,
        cx: &mut Context,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>
    > {
        if let Some(inner) = self.inner.as_mut() {
            inner.poll(cx)
        } else {
            Poll::Pending
        }
    }
}
