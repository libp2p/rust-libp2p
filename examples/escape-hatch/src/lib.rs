use futures::channel::oneshot;
use handler::Connection;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, SubstreamProtocol, THandler, THandlerOutEvent,
    ToSwarm,
};
use libp2p::{swarm::NetworkBehaviour, PeerId, Stream, StreamProtocol};
use libp2p_core::upgrade::ReadyUpgrade;
use libp2p_core::{Endpoint, Multiaddr};
use std::task::Context;
use std::{collections::VecDeque, io, task::Poll, time::Duration};
use std::sync::mpsc;

mod handler;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/ping/1.0.0");

#[derive(Debug)]
pub enum Error {
    NotConnected(PeerId),
    UnsupportedProtocol,
    Io(io::Error),
}

/// A behaviour that manages requests to open new streams which are directly handed to the user.
pub struct Behaviour {
    /// Config
    config: Config,
    /// Queue of events to yield to the swarm.
    events: VecDeque<Event>,
    /// Protocol
    protocol: StreamProtocol,

    receiver: mpsc::Receiver<Event>
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The timeout of an outbound ping.
    timeout: Duration,
    /// The duration between outbound pings.
    interval: Duration,
}

pub struct Event {
    pub peer_id: PeerId, // Assuming PeerId is a type representing a peer
    pub response_channel: oneshot::Sender<Result<Stream, Error>>, // For sending back the result
}

pub struct IncomingStreams {
    /// Queue of events to yield to the swarm.
    events: VecDeque<Event>,
}

/// A control acts as a "remote" that allows users to request streams without interacting with the `Swarm` directly.

pub struct Control {
    sender: mpsc::Sender<Event>,
}

impl IncomingStreams {
    pub async fn next(&mut self) -> (PeerId, Stream) {
        if let Some(e) = self.events.pop_back() {
        } else {
        }

        todo!()
    }

    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<(PeerId, Stream)> {
        todo!()
    }
}

impl Behaviour {
    pub fn new(protocol: StreamProtocol) -> (Self, Control, IncomingStreams) {
        let (sender, receiver) = mpsc::channel::<Event>();
        let behaviour = Self {
            config: Config {
                timeout: Duration::from_secs(1),
                interval: Duration::from_secs(1),
            },
            events: VecDeque::new(),
            protocol,
            receiver
        };

        (
            behaviour,
            Control {
                sender,
            },
            IncomingStreams {
                events: VecDeque::new(),
            },
        )
    }
}

impl Control {
    pub async fn open_stream(&self, peer_id: PeerId) -> Result<Stream, Error> {
        let (response_tx, response_rx) = oneshot::channel();
        let message = Event {
            peer_id,
            response_channel: response_tx,
        };
        // Send the message to NetworkBehaviour
        self.sender.send(message).expect("Failed to send message");
        response_rx.await.expect("Failed to receive response")
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Connection;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Connection::new(self.config.clone()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Connection::new(self.config.clone()))
    }

    fn on_swarm_event(&mut self, _event: FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        peer: PeerId,
        connection: ConnectionId,
        result: THandlerOutEvent<Self>,
    ) {
        todo!()
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        let Ok(message) = self.receiver.recv() else {
            return Poll::Pending;
        };
        let stream = SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_NAME), ());
        
        // message.response_channel.send(t)
        Poll::Ready(ToSwarm::GenerateEvent(message))
    }
}
