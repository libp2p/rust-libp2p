use crate::protocol;
use libp2p_core::{InboundUpgrade, OutboundUpgrade};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use std::error::Error;
use std::fmt;
use std::task::{Context, Poll};

#[derive(Debug)]
pub struct HandlerEvent(protocol::Message);

pub struct RendezvousHandler;

impl RendezvousHandler {
    pub fn new() -> Self {
        Self
    }
}

/// The result of an inbound or outbound ping.
pub type RendezvousResult = Result<RendezvousSuccess, RendezvousFailure>;

/// The successful result of processing an inbound or outbound ping.
#[derive(Debug)]
pub enum RendezvousSuccess {}

/// An outbound ping failure.
#[derive(Debug)]
pub enum RendezvousFailure {
    Other {
        error: Box<dyn std::error::Error + Send + 'static>,
    },
}

impl fmt::Display for RendezvousFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RendezvousFailureFailure::Other { error } => write!(f, "Rendezvous error: {}", error),
        }
    }
}

impl Error for RendezvousFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RendezvousFailure::Other { error } => Some(&**error),
        }
    }
}

impl ProtocolsHandler for RendezvousHandler {
    type InEvent = ();
    type OutEvent = RendezvousResult;
    type Error = RendezvousFailure;
    type InboundProtocol = protocol::Rendezvous;
    type OutboundProtocol = protocol::Rendezvous;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        todo!()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _info: Self::InboundOpenInfo,
    ) {
        // If the peer doesn't support the protocol, reject all substreams
        if self.protocol_unsupported {
            return;
        }

        self.inbound_substreams_created += 1;

        // update the known kind of peer
        if self.peer_kind.is_none() {
            self.peer_kind = Some(peer_kind);
        }

        // new inbound substream. Replace the current one, if it exists.
        trace!("New inbound substream request");
        self.inbound_substream = Some(InboundSubstreamState::WaitingInput(substream));
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        message: Self::OutboundOpenInfo,
    ) {
        if self.outbound_substream.is_some() {
            warn!("Established an outbound substream with one already available");
            // Add the message back to the send queue
            self.send_queue.push(message);
        } else {
            self.outbound_substream = Some(OutboundSubstreamState::PendingSend(substream, message));
        }
    }

    fn inject_event(&mut self, message: GossipsubHandlerIn) {
        if !self.protocol_unsupported {
            match message {
                GossipsubHandlerIn::Message(m) => self.send_queue.push(m),
                // If we have joined the mesh, keep the connection alive.
                GossipsubHandlerIn::JoinedMesh => {
                    self.in_mesh = true;
                    self.keep_alive = KeepAlive::Yes;
                }
                // If we have left the mesh, start the idle timer.
                GossipsubHandlerIn::LeftMesh => {
                    self.in_mesh = false;
                    self.keep_alive = KeepAlive::Until(Instant::now() + self.idle_timeout);
                }
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<()>,
    ) {
        todo!()
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        todo!()
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        todo!()
    }
}
