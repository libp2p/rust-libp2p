use crate::handler::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    SubstreamProtocol,
};
use crate::NegotiatedSubstream;
use libp2p_core::{
    upgrade::{DeniedUpgrade, InboundUpgrade, OutboundUpgrade},
    Multiaddr,
};
use std::task::{Context, Poll};
use void::Void;

/// Implementation of [`ConnectionHandler`] that doesn't handle anything but keeps the connection alive.
#[derive(Clone, Debug)]
pub struct KeepAliveConnectionHandler;

impl ConnectionHandler for KeepAliveConnectionHandler {
    type InEvent = Void;
    type OutEvent = Void;
    type Error = Void;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = Void;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        _: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
        unreachable!("`DeniedUpgrade` is never successful.");
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        _: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        v: Self::OutboundOpenInfo,
    ) {
        void::unreachable(v)
    }

    fn inject_event(&mut self, v: Self::InEvent) {
        void::unreachable(v)
    }

    fn inject_address_change(&mut self, _: &Multiaddr) {}

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ConnectionHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
    }

    fn inject_listen_upgrade_error(
        &mut self,
        _: Self::InboundOpenInfo,
        _: ConnectionHandlerUpgrErr<
            <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::Yes
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        Poll::Pending
    }
}
