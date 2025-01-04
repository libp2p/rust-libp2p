use std::{
    collections::{hash_map::Entry, HashMap},
    io,
    sync::{Arc, Mutex, MutexGuard},
};

use futures::channel::mpsc;
use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionId, Stream, StreamProtocol};
use rand::seq::IteratorRandom as _;

use crate::{handler::NewStream, AlreadyRegistered, IncomingStreams};

pub(crate) struct Shared {
    /// Tracks the supported inbound protocols created via
    /// [`Control::accept`](crate::Control::accept).
    ///
    /// For each [`StreamProtocol`], we hold the [`mpsc::Sender`] corresponding to the
    /// [`mpsc::Receiver`] in [`IncomingStreams`].
    supported_inbound_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,

    connections: HashMap<ConnectionId, PeerId>,
    senders: HashMap<ConnectionId, mpsc::Sender<NewStream>>,

    /// Tracks channel pairs for a peer whilst we are dialing them.
    pending_channels: HashMap<PeerId, (mpsc::Sender<NewStream>, mpsc::Receiver<NewStream>)>,

    /// Sender for peers we want to dial.
    ///
    /// We manage this through a channel to avoid locks as part of
    /// [`NetworkBehaviour::poll`](libp2p_swarm::NetworkBehaviour::poll).
    dial_sender: mpsc::Sender<PeerId>,
}

impl Shared {
    pub(crate) fn lock(shared: &Arc<Mutex<Shared>>) -> MutexGuard<'_, Shared> {
        shared.lock().unwrap_or_else(|e| e.into_inner())
    }
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

        Ok(IncomingStreams::new(receiver))
    }

    /// Lists the protocols for which we have an active [`IncomingStreams`] instance.
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

    pub(crate) fn on_connection_established(&mut self, conn: ConnectionId, peer: PeerId) {
        self.connections.insert(conn, peer);
    }

    pub(crate) fn on_connection_closed(&mut self, conn: ConnectionId) {
        self.connections.remove(&conn);
    }

    pub(crate) fn on_dial_failure(&mut self, peer: PeerId, reason: String) {
        let Some((_, mut receiver)) = self.pending_channels.remove(&peer) else {
            return;
        };

        while let Ok(Some(new_stream)) = receiver.try_next() {
            let _ = new_stream
                .sender
                .send(Err(crate::OpenStreamError::Io(io::Error::new(
                    io::ErrorKind::NotConnected,
                    reason.clone(),
                ))));
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
                tracing::debug!(%peer, "Not connected to peer, initiating dial");

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
