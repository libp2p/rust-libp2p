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

use crate::handler::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, IntoConnectionHandler,
    KeepAlive, SubstreamProtocol,
};
use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend, SendWrapper};
use crate::{DialError, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use either::Either;
use libp2p_core::{
    connection::ConnectionId,
    either::{EitherError, EitherOutput},
    transport::ListenerId,
    upgrade::{DeniedUpgrade, EitherUpgrade},
    ConnectedPoint, Multiaddr, PeerId,
};
use std::{task::Context, task::Poll};

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

    /// Returns a reference to the inner `NetworkBehaviour`.
    pub fn as_ref(&self) -> Option<&TBehaviour> {
        self.inner.as_ref()
    }

    /// Returns a mutable reference to the inner `NetworkBehaviour`.
    pub fn as_mut(&mut self) -> Option<&mut TBehaviour> {
        self.inner.as_mut()
    }
}

impl<TBehaviour> From<Option<TBehaviour>> for Toggle<TBehaviour> {
    fn from(inner: Option<TBehaviour>) -> Self {
        Toggle { inner }
    }
}

impl<TBehaviour> NetworkBehaviour for Toggle<TBehaviour>
where
    TBehaviour: NetworkBehaviour,
{
    type ConnectionHandler = ToggleIntoConnectionHandler<TBehaviour::ConnectionHandler>;
    type OutEvent = TBehaviour::OutEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        ToggleIntoConnectionHandler {
            inner: self.inner.as_mut().map(|i| i.new_handler()),
        }
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.inner
            .as_mut()
            .map(|b| b.addresses_of_peer(peer_id))
            .unwrap_or_else(Vec::new)
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        endpoint: &ConnectedPoint,
        errors: Option<&Vec<Multiaddr>>,
        other_established: usize,
    ) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_connection_established(
                peer_id,
                connection,
                endpoint,
                errors,
                other_established,
            )
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        endpoint: &ConnectedPoint,
        handler: <Self::ConnectionHandler as IntoConnectionHandler>::Handler,
        remaining_established: usize,
    ) {
        if let Some(inner) = self.inner.as_mut() {
            if let Some(handler) = handler.inner {
                inner.inject_connection_closed(
                    peer_id,
                    connection,
                    endpoint,
                    handler,
                    remaining_established,
                )
            }
        }
    }

    fn inject_address_change(
        &mut self,
        peer_id: &PeerId,
        connection: &ConnectionId,
        old: &ConnectedPoint,
        new: &ConnectedPoint,
    ) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_address_change(peer_id, connection, old, new)
        }
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection: ConnectionId,
        event: <<Self::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::OutEvent,
    ) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_event(peer_id, connection, event);
        }
    }

    fn inject_dial_failure(
        &mut self,
        peer_id: Option<PeerId>,
        handler: Self::ConnectionHandler,
        error: &DialError,
    ) {
        if let Some(inner) = self.inner.as_mut() {
            if let Some(handler) = handler.inner {
                inner.inject_dial_failure(peer_id, handler, error)
            }
        }
    }

    fn inject_listen_failure(
        &mut self,
        local_addr: &Multiaddr,
        send_back_addr: &Multiaddr,
        handler: Self::ConnectionHandler,
    ) {
        if let Some(inner) = self.inner.as_mut() {
            if let Some(handler) = handler.inner {
                inner.inject_listen_failure(local_addr, send_back_addr, handler)
            }
        }
    }

    fn inject_new_listener(&mut self, id: ListenerId) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_new_listener(id)
        }
    }

    fn inject_new_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_new_listen_addr(id, addr)
        }
    }

    fn inject_expired_listen_addr(&mut self, id: ListenerId, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_expired_listen_addr(id, addr)
        }
    }

    fn inject_new_external_addr(&mut self, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_new_external_addr(addr)
        }
    }

    fn inject_expired_external_addr(&mut self, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_expired_external_addr(addr)
        }
    }

    fn inject_listener_error(&mut self, id: ListenerId, err: &(dyn std::error::Error + 'static)) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_listener_error(id, err)
        }
    }

    fn inject_listener_closed(&mut self, id: ListenerId, reason: Result<(), &std::io::Error>) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_listener_closed(id, reason)
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if let Some(inner) = self.inner.as_mut() {
            inner.poll(cx, params).map(|action| {
                action.map_handler(|h| ToggleIntoConnectionHandler { inner: Some(h) })
            })
        } else {
            Poll::Pending
        }
    }
}

/// Implementation of `IntoConnectionHandler` that can be in the disabled state.
pub struct ToggleIntoConnectionHandler<TInner> {
    inner: Option<TInner>,
}

impl<TInner> IntoConnectionHandler for ToggleIntoConnectionHandler<TInner>
where
    TInner: IntoConnectionHandler,
{
    type Handler = ToggleConnectionHandler<TInner::Handler>;

    fn into_handler(
        self,
        remote_peer_id: &PeerId,
        connected_point: &ConnectedPoint,
    ) -> Self::Handler {
        ToggleConnectionHandler {
            inner: self
                .inner
                .map(|h| h.into_handler(remote_peer_id, connected_point)),
        }
    }

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
        if let Some(inner) = self.inner.as_ref() {
            EitherUpgrade::A(SendWrapper(inner.inbound_protocol()))
        } else {
            EitherUpgrade::B(SendWrapper(DeniedUpgrade))
        }
    }
}

/// Implementation of [`ConnectionHandler`] that can be in the disabled state.
pub struct ToggleConnectionHandler<TInner> {
    inner: Option<TInner>,
}

impl<TInner> ConnectionHandler for ToggleConnectionHandler<TInner>
where
    TInner: ConnectionHandler,
{
    type InEvent = TInner::InEvent;
    type OutEvent = TInner::OutEvent;
    type Error = TInner::Error;
    type InboundProtocol =
        EitherUpgrade<SendWrapper<TInner::InboundProtocol>, SendWrapper<DeniedUpgrade>>;
    type OutboundProtocol = TInner::OutboundProtocol;
    type OutboundOpenInfo = TInner::OutboundOpenInfo;
    type InboundOpenInfo = Either<TInner::InboundOpenInfo, ()>;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        if let Some(inner) = self.inner.as_ref() {
            inner
                .listen_protocol()
                .map_upgrade(|u| EitherUpgrade::A(SendWrapper(u)))
                .map_info(Either::Left)
        } else {
            SubstreamProtocol::new(
                EitherUpgrade::B(SendWrapper(DeniedUpgrade)),
                Either::Right(()),
            )
        }
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        out: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        info: Self::InboundOpenInfo,
    ) {
        let out = match out {
            EitherOutput::First(out) => out,
            EitherOutput::Second(v) => void::unreachable(v),
        };

        if let Either::Left(info) = info {
            self.inner
                .as_mut()
                .expect("Can't receive an inbound substream if disabled; QED")
                .inject_fully_negotiated_inbound(out, info)
        } else {
            panic!("Unexpected Either::Right in enabled `inject_fully_negotiated_inbound`.")
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        out: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        self.inner
            .as_mut()
            .expect("Can't receive an outbound substream if disabled; QED")
            .inject_fully_negotiated_outbound(out, info)
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        self.inner
            .as_mut()
            .expect("Can't receive events if disabled; QED")
            .inject_event(event)
    }

    fn inject_address_change(&mut self, addr: &Multiaddr) {
        if let Some(inner) = self.inner.as_mut() {
            inner.inject_address_change(addr)
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        err: ConnectionHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
    ) {
        self.inner
            .as_mut()
            .expect("Can't receive an outbound substream if disabled; QED")
            .inject_dial_upgrade_error(info, err)
    }

    fn inject_listen_upgrade_error(
        &mut self,
        info: Self::InboundOpenInfo,
        err: ConnectionHandlerUpgrErr<<Self::InboundProtocol as InboundUpgradeSend>::Error>,
    ) {
        let (inner, info) = match (self.inner.as_mut(), info) {
            (Some(inner), Either::Left(info)) => (inner, info),
            // Ignore listen upgrade errors in disabled state.
            (None, Either::Right(())) => return,
            (Some(_), Either::Right(())) => panic!(
                "Unexpected `Either::Right` inbound info through \
                 `inject_listen_upgrade_error` in enabled state.",
            ),
            (None, Either::Left(_)) => panic!(
                "Unexpected `Either::Left` inbound info through \
                 `inject_listen_upgrade_error` in disabled state.",
            ),
        };

        let err = match err {
            ConnectionHandlerUpgrErr::Timeout => ConnectionHandlerUpgrErr::Timeout,
            ConnectionHandlerUpgrErr::Timer => ConnectionHandlerUpgrErr::Timer,
            ConnectionHandlerUpgrErr::Upgrade(err) => {
                ConnectionHandlerUpgrErr::Upgrade(err.map_err(|err| match err {
                    EitherError::A(e) => e,
                    EitherError::B(v) => void::unreachable(v),
                }))
            }
        };

        inner.inject_listen_upgrade_error(info, err)
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.inner
            .as_ref()
            .map(|h| h.connection_keep_alive())
            .unwrap_or(KeepAlive::No)
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
        if let Some(inner) = self.inner.as_mut() {
            inner.poll(cx)
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dummy;

    /// A disabled [`ToggleConnectionHandler`] can receive listen upgrade errors in
    /// the following two cases:
    ///
    /// 1. Protocol negotiation on an incoming stream failed with no protocol
    ///    being agreed on.
    ///
    /// 2. When combining [`ConnectionHandler`] implementations a single
    ///    [`ConnectionHandler`] might be notified of an inbound upgrade error
    ///    unrelated to its own upgrade logic. For example when nesting a
    ///    [`ToggleConnectionHandler`] in a
    ///    [`ConnectionHandlerSelect`](crate::connection_handler::ConnectionHandlerSelect)
    ///    the former might receive an inbound upgrade error even when disabled.
    ///
    /// [`ToggleConnectionHandler`] should ignore the error in both of these cases.
    #[test]
    fn ignore_listen_upgrade_error_when_disabled() {
        let mut handler = ToggleConnectionHandler::<dummy::ConnectionHandler> { inner: None };

        handler.inject_listen_upgrade_error(Either::Right(()), ConnectionHandlerUpgrErr::Timeout);
    }
}
