use libp2p::swarm::{ConnectionHandler, SubstreamProtocol, ConnectionHandlerEvent};
use libp2p::StreamProtocol;
use libp2p_core::upgrade::ReadyUpgrade;
use std::task::Context;
use std::{task::Poll, time::Duration};
use void::Void;

use crate::{Config, Error, PROTOCOL_NAME};

pub struct Connection {
    config: Config,
}

impl Connection {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl ConnectionHandler for Connection {
    type FromBehaviour = Void;

    type ToBehaviour = Result<Duration, Error>;

    type InboundProtocol = ReadyUpgrade<StreamProtocol>;

    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;

    type InboundOpenInfo = ();

    type OutboundOpenInfo = ();

    fn listen_protocol(
        &self,
    ) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, _: Void) {}

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        libp2p::swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        let protocol = self.listen_protocol();
        return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
            protocol,
        });
    }

    fn on_connection_event(
        &mut self,
        event: libp2p::swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        todo!()
    }
}
