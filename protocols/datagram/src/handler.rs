use std::{
    collections::VecDeque,
    convert::Infallible,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::channel::mpsc;
use libp2p_core::upgrade::DeniedUpgrade;
use libp2p_identity::PeerId;
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, SubstreamProtocol, handler::ConnectionEvent,
};

pub struct Handler {
    remote: PeerId,
    inbound: mpsc::Sender<(PeerId, Bytes)>,
    outbound: VecDeque<Bytes>,
}

impl Handler {
    pub(crate) fn new(remote: PeerId, inbound: mpsc::Sender<(PeerId, Bytes)>) -> Self {
        Self {
            remote,
            inbound,
            outbound: VecDeque::new(),
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Bytes;
    type ToBehaviour = Infallible;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        match self.outbound.pop_front() {
            Some(data) => Poll::Ready(ConnectionHandlerEvent::SendDatagram(data)),
            None => Poll::Pending,
        }
    }

    fn on_behaviour_event(&mut self, data: Self::FromBehaviour) {
        self.outbound.push_back(data);
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        if let ConnectionEvent::Datagram(datagram) = event {
            let _ = self.inbound.try_send((self.remote, datagram.data.clone()));
        }
    }
}
