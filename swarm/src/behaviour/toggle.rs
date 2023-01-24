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

use crate::behaviour::FromSwarm;
use crate::connection::ConnectionId;
use crate::handler::{
    AddressChange, ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent,
    ConnectionHandlerUpgrErr, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
    IntoConnectionHandler, KeepAlive, ListenUpgradeError, SubstreamProtocol,
};
use crate::upgrade::SendWrapper;
use crate::{NetworkBehaviour, NetworkBehaviourAction, PollParameters, THandlerOutEvent};
use either::Either;
use futures::future;
use libp2p_core::{upgrade::DeniedUpgrade, ConnectedPoint, Multiaddr, PeerId};
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

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        if let Some(behaviour) = &mut self.inner {
            if let Some(event) = event.maybe_map_handler(|h| h.inner, |h| h.inner) {
                behaviour.on_swarm_event(event);
            }
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        if let Some(behaviour) = &mut self.inner {
            behaviour.on_connection_handler_event(peer_id, connection_id, event)
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
            Either::Left(SendWrapper(inner.inbound_protocol()))
        } else {
            Either::Right(SendWrapper(DeniedUpgrade))
        }
    }
}

/// Implementation of [`ConnectionHandler`] that can be in the disabled state.
pub struct ToggleConnectionHandler<TInner> {
    inner: Option<TInner>,
}

impl<TInner> ToggleConnectionHandler<TInner>
where
    TInner: ConnectionHandler,
{
    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: out,
            info,
        }: FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
        let out = match out {
            future::Either::Left(out) => out,
            future::Either::Right(v) => void::unreachable(v),
        };

        if let Either::Left(info) = info {
            self.inner
                .as_mut()
                .expect("Can't receive an inbound substream if disabled; QED")
                .on_connection_event(ConnectionEvent::FullyNegotiatedInbound(
                    FullyNegotiatedInbound {
                        protocol: out,
                        info,
                    },
                ));
        } else {
            panic!("Unexpected Either::Right in enabled `on_fully_negotiated_inbound`.")
        }
    }

    fn on_listen_upgrade_error(
        &mut self,
        ListenUpgradeError { info, error: err }: ListenUpgradeError<
            <Self as ConnectionHandler>::InboundOpenInfo,
            <Self as ConnectionHandler>::InboundProtocol,
        >,
    ) {
        let (inner, info) = match (self.inner.as_mut(), info) {
            (Some(inner), Either::Left(info)) => (inner, info),
            // Ignore listen upgrade errors in disabled state.
            (None, Either::Right(())) => return,
            (Some(_), Either::Right(())) => panic!(
                "Unexpected `Either::Right` inbound info through \
                 `on_listen_upgrade_error` in enabled state.",
            ),
            (None, Either::Left(_)) => panic!(
                "Unexpected `Either::Left` inbound info through \
                 `on_listen_upgrade_error` in disabled state.",
            ),
        };

        let err = match err {
            ConnectionHandlerUpgrErr::Timeout => ConnectionHandlerUpgrErr::Timeout,
            ConnectionHandlerUpgrErr::Timer => ConnectionHandlerUpgrErr::Timer,
            ConnectionHandlerUpgrErr::Upgrade(err) => {
                ConnectionHandlerUpgrErr::Upgrade(err.map_err(|err| match err {
                    Either::Left(e) => e,
                    Either::Right(v) => void::unreachable(v),
                }))
            }
        };

        inner.on_connection_event(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
            info,
            error: err,
        }));
    }
}

impl<TInner> ConnectionHandler for ToggleConnectionHandler<TInner>
where
    TInner: ConnectionHandler,
{
    type InEvent = TInner::InEvent;
    type OutEvent = TInner::OutEvent;
    type Error = TInner::Error;
    type InboundProtocol = Either<SendWrapper<TInner::InboundProtocol>, SendWrapper<DeniedUpgrade>>;
    type OutboundProtocol = TInner::OutboundProtocol;
    type OutboundOpenInfo = TInner::OutboundOpenInfo;
    type InboundOpenInfo = Either<TInner::InboundOpenInfo, ()>;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        if let Some(inner) = self.inner.as_ref() {
            inner
                .listen_protocol()
                .map_upgrade(|u| Either::Left(SendWrapper(u)))
                .map_info(Either::Left)
        } else {
            SubstreamProtocol::new(Either::Right(SendWrapper(DeniedUpgrade)), Either::Right(()))
        }
    }

    fn on_behaviour_event(&mut self, event: Self::InEvent) {
        self.inner
            .as_mut()
            .expect("Can't receive events if disabled; QED")
            .on_behaviour_event(event)
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

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                self.on_fully_negotiated_inbound(fully_negotiated_inbound)
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: out,
                info,
            }) => self
                .inner
                .as_mut()
                .expect("Can't receive an outbound substream if disabled; QED")
                .on_connection_event(ConnectionEvent::FullyNegotiatedOutbound(
                    FullyNegotiatedOutbound {
                        protocol: out,
                        info,
                    },
                )),
            ConnectionEvent::AddressChange(address_change) => {
                if let Some(inner) = self.inner.as_mut() {
                    inner.on_connection_event(ConnectionEvent::AddressChange(AddressChange {
                        new_address: address_change.new_address,
                    }));
                }
            }
            ConnectionEvent::DialUpgradeError(DialUpgradeError { info, error: err }) => self
                .inner
                .as_mut()
                .expect("Can't receive an outbound substream if disabled; QED")
                .on_connection_event(ConnectionEvent::DialUpgradeError(DialUpgradeError {
                    info,
                    error: err,
                })),
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            }
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

        handler.on_connection_event(ConnectionEvent::ListenUpgradeError(ListenUpgradeError {
            info: Either::Right(()),
            error: ConnectionHandlerUpgrErr::Timeout,
        }));
    }
}
