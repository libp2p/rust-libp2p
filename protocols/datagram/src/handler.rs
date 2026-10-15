use std::{
    collections::VecDeque,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::channel::mpsc;
use libp2p_core::Endpoint;
use libp2p_identity::PeerId;
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, Stream, StreamProtocol, SubstreamProtocol,
    handler::{ConnectionEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound},
};

use crate::{
    framing,
    protocol::{CONTROL_PROTOCOL, Upgrade},
};

/// Pre-establish backlog cap; head-drop oldest past it (delivery is unreliable).
const MAX_OUTBOUND_BACKLOG: usize = 256;

/// Drives one datagram flow: the dialer opens the `/dg/1` control stream, both
/// ends learn its stream id, and datagrams are framed and filtered by it.
pub struct Handler {
    remote: PeerId,
    role: Endpoint,
    protocol: StreamProtocol,
    inbound: mpsc::Sender<(PeerId, Bytes)>,
    requested_control_stream: bool,
    control_stream_id: Option<u64>,
    // Held open for the life of the flow, per spec; never read or written again.
    control_stream: Option<Stream>,
    outbound: VecDeque<Bytes>,
    reported_max: Option<usize>,
    pending_max: Option<usize>,
}

impl Handler {
    pub(crate) fn new(
        remote: PeerId,
        role: Endpoint,
        protocol: StreamProtocol,
        inbound: mpsc::Sender<(PeerId, Bytes)>,
    ) -> Self {
        Self {
            remote,
            role,
            protocol,
            inbound,
            requested_control_stream: false,
            control_stream_id: None,
            control_stream: None,
            outbound: VecDeque::new(),
            reported_max: None,
            pending_max: None,
        }
    }

    fn upgrade(&self) -> SubstreamProtocol<Upgrade, ()> {
        SubstreamProtocol::new(
            Upgrade {
                application_protocol: self.protocol.clone(),
            },
            (),
        )
    }

    fn establish(&mut self, stream: Stream) {
        self.control_stream_id = stream.transport_stream_id();
        self.control_stream = Some(stream);
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Bytes;
    type ToBehaviour = usize;
    type InboundProtocol = Upgrade;
    type OutboundProtocol = Upgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Upgrade> {
        self.upgrade()
    }

    fn supports_datagrams(&self, protocol: &StreamProtocol) -> bool {
        *protocol == CONTROL_PROTOCOL
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ConnectionHandlerEvent<Upgrade, (), usize>> {
        if let Some(max) = self.pending_max.take() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(max));
        }

        if self.role == Endpoint::Dialer && !self.requested_control_stream {
            self.requested_control_stream = true;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: self.upgrade(),
            });
        }

        if let Some(id) = self.control_stream_id
            && let Some(payload) = self.outbound.pop_front()
        {
            return Poll::Ready(ConnectionHandlerEvent::SendDatagram(framing::frame(
                id, &payload,
            )));
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, data: Bytes) {
        if self.outbound.len() >= MAX_OUTBOUND_BACKLOG {
            self.outbound.pop_front();
        }
        self.outbound.push_back(data);
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: stream,
                ..
            })
            | ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: stream,
                ..
            }) => self.establish(stream),
            ConnectionEvent::Datagram(datagram) => {
                // Already parsed and routed to us by the connection, id stripped.
                let _ = self.inbound.try_send((self.remote, datagram.data.clone()));
            }
            ConnectionEvent::DatagramMaxSize(max) if self.reported_max != Some(max) => {
                self.reported_max = Some(max);
                self.pending_max = Some(max);
            }
            _ => {}
        }
    }
}
