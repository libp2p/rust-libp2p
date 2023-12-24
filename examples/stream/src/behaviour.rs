use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    task::{Context, Poll},
};

use flume::r#async::SendSink;
use futures::{
    channel::{mpsc, oneshot},
    StreamExt as _,
};
use libp2p::{
    swarm::{
        self, behaviour::ConnectionEstablished, dial_opts::DialOpts, ConnectionClosed,
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId, Stream, StreamProtocol,
};

use crate::{
    handler::{Handler, NewStream, RegisterProtocol, ToHandler},
    Control, IncomingStreams,
};

pub struct Behaviour {
    sender: mpsc::Sender<NewPeerControl>,
    receiver: mpsc::Receiver<NewPeerControl>,

    supported_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,
    active_connections: HashSet<(PeerId, ConnectionId)>,

    // Note: Connections will perform work-stealing on answering `NewStream` messages.
    connections_by_peer_id: HashMap<PeerId, (flume::Sender<NewStream>, flume::Receiver<NewStream>)>,

    events: VecDeque<ToSwarm<(), ToHandler>>,

    pending_connections:
        HashMap<PeerId, Vec<oneshot::Sender<io::Result<SendSink<'static, NewStream>>>>>,
}

impl Default for Behaviour {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel(0);

        Self {
            sender,
            receiver,
            connections_by_peer_id: HashMap::default(),
            active_connections: HashSet::default(),
            events: VecDeque::default(),
            supported_protocols: HashMap::default(),
            pending_connections: HashMap::default(),
        }
    }
}

impl Behaviour {
    pub fn register(
        &mut self,
        protocol: StreamProtocol,
    ) -> Result<(Control, IncomingStreams), AlreadyRegistered> {
        if self.supported_protocols.contains_key(&protocol) {
            return Err(AlreadyRegistered);
        }

        let (sender, receiver) = mpsc::channel(10);
        self.supported_protocols
            .insert(protocol.clone(), sender.clone());

        self.events
            .extend(
                self.active_connections
                    .iter()
                    .map(|(peer, conn)| ToSwarm::NotifyHandler {
                        peer_id: *peer,
                        handler: swarm::NotifyHandler::One(*conn),
                        event: ToHandler::RegisterProtocol(RegisterProtocol {
                            protocol: protocol.clone(),
                            sender: sender.clone(),
                        }),
                    }),
            );

        tracing::debug!(
            %protocol,
            "Registering protocol with {} existing handlers",
            self.active_connections.len()
        );

        Ok((
            Control {
                sender: self.sender.clone(),
                protocol: protocol,
            },
            IncomingStreams { receiver },
        ))
    }
}

#[derive(Debug)]
pub struct AlreadyRegistered;

/// Message from a [`Control`] to the [`Behaviour`] to construct a new [`PeerControl`].
pub(crate) struct NewPeerControl {
    pub(crate) peer: PeerId,
    pub(crate) sender: oneshot::Sender<io::Result<SendSink<'static, NewStream>>>,
}

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
        let (_, receiver) = self
            .connections_by_peer_id
            .entry(peer)
            .or_insert_with(|| flume::bounded(10));

        self.active_connections.insert((peer, connection_id));

        Ok(Handler::new(
            peer,
            self.supported_protocols.clone(),
            receiver.clone().into_stream(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        let (_, receiver) = self
            .connections_by_peer_id
            .entry(peer)
            .or_insert_with(|| flume::bounded(10));

        self.active_connections.insert((peer, connection_id));

        Ok(Handler::new(
            peer,
            self.supported_protocols.clone(),
            receiver.clone().into_stream(),
        ))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished { peer_id, .. }) => {
                let (new_stream_sender, _) = self
                    .connections_by_peer_id
                    .get(&peer_id)
                    .expect("inconsistent state");

                for pending_sender in self
                    .pending_connections
                    .entry(peer_id)
                    .or_default()
                    .drain(..)
                {
                    let _ = pending_sender.send(Ok(new_stream_sender.clone().into_sink()));
                }
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                remaining_established,
                ..
            }) => {
                self.active_connections.remove(&(peer_id, connection_id));

                // If the last connection goes away, clean up the state.
                if remaining_established == 0 {
                    self.connections_by_peer_id.remove(&peer_id);
                }
            }
            // TODO: Handle `DialFailure`
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
        loop {
            match self.receiver.poll_next_unpin(cx) {
                Poll::Ready(Some(NewPeerControl { peer, sender })) => {
                    match self.connections_by_peer_id.get(&peer) {
                        Some((new_stream_sender, _)) => {
                            let _ = sender.send(Ok(new_stream_sender.clone().into_sink()));
                        }
                        None => {
                            self.pending_connections
                                .entry(peer)
                                .or_default()
                                .push(sender);
                            return Poll::Ready(ToSwarm::Dial {
                                opts: DialOpts::peer_id(peer).build(),
                            });
                        }
                    }

                    continue;
                }
                Poll::Ready(None) => unreachable!("we own both sender and receiver"),
                Poll::Pending => return Poll::Pending,
            };
        }
    }
}
