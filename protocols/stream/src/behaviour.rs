use core::fmt;
use std::{
    collections::{hash_map::Entry, HashMap},
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
    handler::{Handler, NewStream},
    Control, IncomingStreams,
};

/// A generic behaviour for stream-oriented protocols.
pub struct Behaviour {
    shared: Arc<Mutex<Shared>>,
    dial_receiver: mpsc::Receiver<PeerId>,
}

pub(crate) struct Shared {
    /// Tracks the supported inbound protocols created via [`Control::accept`].
    ///
    /// For each [`StreamProtocol`], we hold the [`mpsc::Sender`] corresponding to the [`mpsc::Receiver`] in [`IncomingStreams`].
    supported_inbound_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,
    connections: HashMap<ConnectionId, PeerId>,
    senders: HashMap<ConnectionId, mpsc::Sender<NewStream>>,

    pending_channels: HashMap<PeerId, (mpsc::Sender<NewStream>, mpsc::Receiver<NewStream>)>,

    dial_sender: mpsc::Sender<PeerId>,
}

impl Shared {
    pub(crate) fn new(dial_sender: mpsc::Sender<PeerId>) -> Self {
        Self {
            dial_sender,
            connections: Default::default(),
            senders: Default::default(),
            pending_channels: Default::default(),
            supported_inbound_protocols: Default::default(),
        }
    }

    pub(crate) fn accept(
        &mut self,
        protocol: StreamProtocol,
    ) -> Result<IncomingStreams, AlreadyRegistered> {
        if self.supported_inbound_protocols.contains_key(&protocol) {
            return Err(AlreadyRegistered);
        }

        let (sender, receiver) = mpsc::channel(0);
        self.supported_inbound_protocols
            .insert(protocol.clone(), sender);

        Ok(IncomingStreams { receiver })
    }

    /// Lists the protocols for which we have an active [`IncomingStream`]s instance.
    pub(crate) fn supported_inbound_protocols(&mut self) -> Vec<StreamProtocol> {
        self.supported_inbound_protocols
            .retain(|_, sender| !sender.is_closed());

        self.supported_inbound_protocols.keys().cloned().collect()
    }

    pub(crate) fn on_inbound_stream(
        &mut self,
        remote: PeerId,
        stream: Stream,
        protocol: StreamProtocol,
    ) {
        match self.supported_inbound_protocols.entry(protocol.clone()) {
            Entry::Occupied(mut entry) => match entry.get_mut().try_send((remote, stream)) {
                Ok(()) => {}
                Err(e) if e.is_full() => {
                    tracing::debug!(%protocol, "Channel is full, dropping inbound stream");
                }
                Err(e) if e.is_disconnected() => {
                    tracing::debug!(%protocol, "Channel is gone, dropping inbound stream");
                    entry.remove();
                }
                _ => unreachable!(),
            },
            Entry::Vacant(_) => {
                tracing::debug!(%protocol, "channel is gone, dropping inbound stream");
            }
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

    pub(crate) fn receiver(
        &mut self,
        peer: PeerId,
        connection: ConnectionId,
    ) -> mpsc::Receiver<NewStream> {
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
        let (dial_sender, dial_receiver) = mpsc::channel(0);

        Self {
            shared: Arc::new(Mutex::new(Shared::new(dial_sender))),
            dial_receiver,
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
        Ok(Handler::new(
            peer,
            self.shared.clone(),
            self.shared.lock().unwrap().receiver(peer, connection_id),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(
            peer,
            self.shared.clone(),
            self.shared.lock().unwrap().receiver(peer, connection_id),
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
                                error.to_string(), // We can only forward the string repr but it is better than nothing.
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
        if let Poll::Ready(Some(peer)) = self.dial_receiver.poll_next_unpin(cx) {
            return Poll::Ready(ToSwarm::Dial {
                opts: DialOpts::peer_id(peer)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .build(),
            });
        }

        Poll::Pending
    }
}
