use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use libp2p_core::{transport::PortUse, upgrade::DeniedUpgrade, Endpoint, Multiaddr};
use libp2p_identity::PeerId;

use crate::{
    behaviour::{FromSwarm, NetworkBehaviour, ToSwarm},
    connection::ConnectionId,
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound},
    ConnectionDenied, ConnectionHandlerEvent, StreamUpgradeError, SubstreamProtocol, THandler,
    THandlerInEvent, THandlerOutEvent,
};

/// Implementation of [`NetworkBehaviour`] that doesn't do anything.
pub struct Behaviour;

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(ConnectionHandler)
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        libp2p_core::util::unreachable(event)
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }

    fn on_swarm_event(&mut self, _event: FromSwarm) {}
}

/// An implementation of [`ConnectionHandler`] that neither handles any protocols nor does it keep
/// the connection alive.
#[derive(Clone)]
pub struct ConnectionHandler;

impl crate::handler::ConnectionHandler for ConnectionHandler {
    type FromBehaviour = Infallible;
    type ToBehaviour = Infallible;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        libp2p_core::util::unreachable(event)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol, ..
            }) => libp2p_core::util::unreachable(protocol),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => libp2p_core::util::unreachable(protocol),
            ConnectionEvent::DialUpgradeError(DialUpgradeError { info: _, error }) => match error {
                StreamUpgradeError::Timeout => unreachable!(),
                StreamUpgradeError::Apply(e) => libp2p_core::util::unreachable(e),
                StreamUpgradeError::NegotiationFailed | StreamUpgradeError::Io(_) => {
                    unreachable!("Denied upgrade does not support any protocols")
                }
            },
            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::ListenUpgradeError(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_) => {}
        }
    }
}
