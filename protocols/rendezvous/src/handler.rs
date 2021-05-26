use crate::codec::Message;
use crate::codec::RendezvousCodec;
use crate::protocol;
use asynchronous_codec::Framed;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::task::{Context, Poll};
use std::time::Instant;

pub struct RendezvousHandler {
    /// Upgrade configuration for the rendezvous protocol.
    listen_protocol: SubstreamProtocol<protocol::Rendezvous, ()>,

    in_events: VecDeque<RendezvousHandlerIn>,

    /// Queue of values that we want to send to the remote.
    out_events: VecDeque<RendezvousHandlerOut>,
}

impl RendezvousHandler {
    pub fn new() -> Self {
        Self {
            listen_protocol: SubstreamProtocol::new(Default::default(), ()),
            in_events: VecDeque::new(),
            out_events: VecDeque::new(),
        }
    }
}

#[derive(Debug)]
pub enum RendezvousHandlerOut {
    DiscoverResponse(DiscoverResponse),
    RegisterResponse,
    RegisterRequest,
    DiscoverRequest,
}

#[derive(Debug, Clone)]
pub enum RendezvousHandlerIn {
    DiscoverRequest,
    RegisterRequest,
}

impl ProtocolsHandler for RendezvousHandler {
    type InEvent = RendezvousHandlerIn;
    type OutEvent = RendezvousHandlerOut;
    type Error = ();
    type InboundOpenInfo = ();
    type InboundProtocol = protocol::Rendezvous;
    type OutboundOpenInfo = ();
    type OutboundProtocol = protocol::Rendezvous;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        let rendezvous_protocol = crate::protocol::Rendezvous::new();
        SubstreamProtocol::new(rendezvous_protocol, ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _info: Self::InboundOpenInfo,
    ) {
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        _info: Self::OutboundOpenInfo,
    ) {
    }

    fn inject_event(&mut self, message: RendezvousHandlerIn) {}

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
