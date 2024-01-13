use core::fmt;
use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use futures::{channel::mpsc, StreamExt};
use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    self as swarm, dial_opts::DialOpts, ConnectionDenied, ConnectionId, FromSwarm,
    NetworkBehaviour, Stream, StreamProtocol, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use rand::seq::IteratorRandom;
use swarm::{
    behaviour::ConnectionEstablished, dial_opts::PeerCondition, ConnectionClosed, DialError,
    DialFailure,
};

use crate::{
    handler::{Handler, NewStream, RegisterProtocol},
    Control, IncomingStreams,
};

/// A generic behaviour for stream-oriented protocols.
pub struct Behaviour {
    shared: Arc<Mutex<Shared>>,
    receiver: mpsc::Receiver<PeerId>,
    supported_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,

    events: VecDeque<ToSwarm<(), RegisterProtocol>>,
}

pub(crate) struct Shared {
    connections: HashMap<ConnectionId, PeerId>,
    senders: HashMap<ConnectionId, mpsc::Sender<NewStream>>,

    pending_channels: HashMap<PeerId, (mpsc::Sender<NewStream>, mpsc::Receiver<NewStream>)>,

    dial_sender: mpsc::Sender<PeerId>,
}

impl Shared {
    pub(crate) fn new(dial_sender: mpsc::Sender<PeerId>) -> Self {
        Self {
            connections: Default::default(),
            senders: Default::default(),
            pending_channels: Default::default(),
            dial_sender,
        }
    }

    pub(crate) fn sender(&mut self, peer: PeerId) -> mpsc::Sender<NewStream> {
        let maybe_sender = self
            .connections
            .iter()
            .filter_map(|(c, p)| (p == &peer).then_some(c))
            .choose(&mut rand::thread_rng())
            .and_then(|c| self.senders.get(c));

        match maybe_sender {
            Some(sender) => {
                tracing::debug!("Returning sender to existing connection");

                sender.clone()
            }
            None => {
                let (sender, _) = self
                    .pending_channels
                    .entry(peer)
                    .or_insert_with(|| mpsc::channel(0));

                let _ = self.dial_sender.try_send(peer);

                sender.clone()
            }
        }
    }

    fn receiver(&mut self, peer: PeerId, connection: ConnectionId) -> mpsc::Receiver<NewStream> {
        if let Some((sender, receiver)) = self.pending_channels.remove(&peer) {
            tracing::debug!(%peer, %connection, "Returning existing pending receiver");

            self.senders.insert(connection, sender);
            return receiver;
        }

        tracing::debug!(%peer, %connection, "Creating new channel pair");

        let (sender, receiver) = mpsc::channel(0);
        self.senders.insert(connection, sender);

        receiver
    }
}

impl Behaviour {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(0);

        Self {
            shared: Arc::new(Mutex::new(Shared::new(sender))),
            receiver,
            supported_protocols: Default::default(),
            events: Default::default(),
        }
    }

    /// Obtain a new [`Control`] for the provided protocol.
    ///
    /// A [`Control`] only deals with the _outbound_ side of a protocol.
    /// To accept inbound streams for a protocol, use [`Behaviour::accept`].
    pub fn new_control(&self) -> Control {
        Control {
            shared: self.shared.clone(),
        }
    }

    /// Accept inbound streams for the provided protocol.
    ///
    /// To stop accepting streams, simply drop the returned [`IncomingStreams`] handle.
    pub fn accept(
        &mut self,
        protocol: StreamProtocol,
    ) -> Result<IncomingStreams, AlreadyRegistered> {
        if self.supported_protocols.contains_key(&protocol) {
            return Err(AlreadyRegistered);
        }

        let (sender, receiver) = mpsc::channel(0);
        self.supported_protocols
            .insert(protocol.clone(), sender.clone());

        let connections = self.shared.lock().unwrap().connections.clone();
        let num_handlers = connections.len();

        self.events.extend(
            connections
                .into_iter()
                .map(|(conn, peer_id)| ToSwarm::NotifyHandler {
                    peer_id,
                    handler: swarm::NotifyHandler::One(conn),
                    event: RegisterProtocol {
                        protocol: protocol.clone(),
                        sender: sender.clone(),
                    },
                }),
        );

        tracing::debug!(
            %protocol,
            "Registering protocol with {num_handlers} existing handlers"
        );

        Ok(IncomingStreams { receiver })
    }
}

/// The protocol is already registered.
#[derive(Debug)]
pub struct AlreadyRegistered;

impl fmt::Display for AlreadyRegistered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "The protocol is already registered")
    }
}

impl std::error::Error for AlreadyRegistered {}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;

    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let receiver = self.shared.lock().unwrap().receiver(peer, connection_id);

        Ok(Handler::new(
            peer,
            self.supported_protocols.clone(),
            receiver,
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let receiver = self.shared.lock().unwrap().receiver(peer, connection_id);

        Ok(Handler::new(
            peer,
            self.supported_protocols.clone(),
            receiver,
        ))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                ..
            }) => {
                self.shared
                    .lock()
                    .unwrap()
                    .connections
                    .insert(connection_id, peer_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed { connection_id, .. }) => {
                self.shared
                    .lock()
                    .unwrap()
                    .connections
                    .remove(&connection_id);
            }
            FromSwarm::DialFailure(DialFailure {
                peer_id: Some(peer_id),
                error:
                    error @ (DialError::Transport(_)
                    | DialError::Denied { .. }
                    | DialError::NoAddresses
                    | DialError::WrongPeerId { .. }),
                ..
            }) => {
                let Some((_, mut receiver)) = self
                    .shared
                    .lock()
                    .unwrap()
                    .pending_channels
                    .remove(&peer_id)
                else {
                    return;
                };

                while let Ok(Some(new_stream)) = receiver.try_next() {
                    let _ =
                        new_stream
                            .sender
                            .send(Err(crate::OpenStreamError::Io(io::Error::new(
                                io::ErrorKind::NotConnected,
                                error.to_string(),
                            ))));
                }
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        void::unreachable(event);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Poll::Ready(Some(peer)) = self.receiver.poll_next_unpin(cx) {
            return Poll::Ready(ToSwarm::Dial {
                opts: DialOpts::peer_id(peer)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .build(),
            });
        }

        Poll::Pending
    }
}
