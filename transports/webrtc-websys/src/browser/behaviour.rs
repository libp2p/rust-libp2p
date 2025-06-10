use std::{
    collections::VecDeque,
    task::Poll,
};

use libp2p_core::PeerId;
use libp2p_swarm::{NetworkBehaviour, NotifyHandler, ToSwarm};

use crate::browser::handler::{FromBehaviourEvent, SignalingHandler, ToBehaviourEvent};

/// Signaling events returned to the swarm.
#[derive(Debug)]
pub enum SignalingEvent {
    NewWebRTCConnection(crate::Connection),
    WebRTCConnectionError(crate::Error),
}

/// A [`Behaviour`] used to cooordinate signaling between peers 
/// over a relay connection.
pub struct Behaviour {
    // Queued events to send to the swarm
    pub(crate) queued_events: VecDeque<ToSwarm<SignalingEvent, FromBehaviourEvent>>
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = SignalingHandler;
    type ToSwarm = SignalingEvent;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        peer: PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(SignalingHandler::new(
            peer,
            false
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        peer: PeerId,
        _addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(SignalingHandler::new(
            peer,
            true
        ))
    }

    fn on_swarm_event(&mut self, event: libp2p_swarm::FromSwarm) {
        match event {
            libp2p_swarm::FromSwarm::ConnectionEstablished(connection_established) => {
               let dst_peer = connection_established.peer_id;
               let endpoint = connection_established.endpoint;

               // Check to see if the connected was made over a relay. If so, we know this is
               // the connection we need to initiate the signaling protocol. We assume the connection
               // was established under a 'dialing' context so we can immediately request a substream over the conn. 
               // For incoming connections we can wait until there is a fully negotiated incoming conn and 
               // start signaling immediately as a non initiator.
               if endpoint.is_relayed() {
                self.queued_events.push_back(ToSwarm::NotifyHandler {
                    peer_id: dst_peer,
                    handler: NotifyHandler::One(connection_established.connection_id),
                    event: FromBehaviourEvent::OpenSignalingStream(dst_peer),
                });
               }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: libp2p_swarm::ConnectionId,
        event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            ToBehaviourEvent::WebRTCConnectionSuccess(connection) => {
                self.queued_events.push_back(ToSwarm::GenerateEvent(
                    SignalingEvent::NewWebRTCConnection(connection)
                ));
            }
            ToBehaviourEvent::WebRTCConnectionFailure(error) => {
                tracing::error!("WebRTC connection failed: {:?}", error);
            }
        }
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        // Check if there are any queued events to send to the swarm
        if !self.queued_events.is_empty() {
            if let Some(event) = self.queued_events.pop_front() {
                return Poll::Ready(event);
            }
        }

        Poll::Pending
    }
}
