use libp2p::{
    swarm::{dummy, NetworkBehaviour},
    PeerId, Stream, StreamProtocol,
};

#[derive(Default)]
pub struct Behaviour {}

impl Behaviour {
    pub fn register(&mut self, _protocol: StreamProtocol) -> (Control, IncomingStreams) {
        todo!()
    }
}

#[derive(Clone)]
pub struct Control {}

impl Control {
    // TOOD: Things that we could make configurable:
    // Timeout for opening a new stream
    pub async fn open_stream(&mut self, _peer: PeerId) -> Result<Stream, Error> {
        todo!()
    }
}

#[derive(Debug)]
pub enum Error {
    /// The remote does not support the requested protocol.
    UnsupportedProtocol,
    /// IO Error that occurred during the protocol handshake.
    Io(std::io::Error),
}

pub struct IncomingStreams {}

impl IncomingStreams {
    pub async fn next(&mut self) -> (PeerId, Stream) {
        todo!()
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;

    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: libp2p::PeerId,
        _local_addr: &libp2p::Multiaddr,
        _remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        todo!()
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: libp2p::PeerId,
        _addr: &libp2p::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        todo!()
    }

    fn on_swarm_event(&mut self, __event: libp2p::swarm::FromSwarm) {
        todo!()
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p::PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        _event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        todo!()
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        todo!()
    }
}
