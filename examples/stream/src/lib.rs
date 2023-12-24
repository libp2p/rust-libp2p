use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    task::{Context, Poll},
    vec,
};

use futures::{
    channel::{mpsc, oneshot},
    future::{self, ready},
    SinkExt, StreamExt,
};
use libp2p::{
    core::UpgradeInfo,
    swarm::{
        self,
        behaviour::ConnectionEstablished,
        dial_opts::DialOpts,
        handler::{
            ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        },
        ConnectionClosed, ConnectionHandler, ConnectionId, FromSwarm, NetworkBehaviour,
        THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    InboundUpgrade, Multiaddr, OutboundUpgrade, PeerId, Stream, StreamProtocol,
};

pub struct Behaviour {
    sender: mpsc::Sender<NewPeerControl>,
    receiver: mpsc::Receiver<NewPeerControl>,

    supported_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,
    active_connections: HashSet<(PeerId, ConnectionId)>,

    // Note: Connections will perform work-stealing on answering `NewStream` messages.
    connections_by_peer_id: HashMap<PeerId, (flume::Sender<NewStream>, flume::Receiver<NewStream>)>,

    events: VecDeque<ToSwarm<(), ToHandler>>,

    pending_connections: HashMap<
        PeerId,
        Vec<oneshot::Sender<io::Result<flume::r#async::SendSink<'static, NewStream>>>>,
    >,
}

pub struct Handler {
    remote: PeerId,
    supported_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,

    receiver: flume::r#async::RecvStream<'static, NewStream>,
    pending_upgrade: Option<(StreamProtocol, oneshot::Sender<Result<Stream, Error>>)>,
}

impl Handler {
    fn new(
        remote: PeerId,
        supported_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,
        receiver: flume::r#async::RecvStream<'static, NewStream>,
    ) -> Self {
        Self {
            supported_protocols,
            receiver,
            pending_upgrade: None,
            remote,
        }
    }
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

// TODO: On `Drop`, we need to de-register the protocol.
#[derive(Clone)]
pub struct Control {
    protocol: StreamProtocol,
    sender: mpsc::Sender<NewPeerControl>,
}

/// Message from a [`Control`] to the [`Behaviour`] to construct a new [`PeerControl`].
struct NewPeerControl {
    peer: PeerId,
    sender: oneshot::Sender<io::Result<flume::r#async::SendSink<'static, NewStream>>>,
}

/// Message from a [`PeerControl`] to a [`ConnectionHandler`] to negotiate a new outbound stream.
struct NewStream {
    protocol: StreamProtocol,
    sender: oneshot::Sender<Result<Stream, Error>>,
}

enum ToHandler {
    RegisterProtocol(RegisterProtocol),
}

#[derive(Debug)]
pub struct RegisterProtocol {
    protocol: StreamProtocol,
    sender: mpsc::Sender<(PeerId, Stream)>,
}

impl Control {
    pub async fn peer(&mut self, peer: PeerId) -> io::Result<PeerControl> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(NewPeerControl { peer, sender })
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionReset, e))?;
        let new_stream_sink = receiver
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionReset, e))??;

        Ok(PeerControl {
            protocol: self.protocol.clone(),
            sender: new_stream_sink,
        })
    }
}

pub struct PeerControl {
    protocol: StreamProtocol,
    sender: flume::r#async::SendSink<'static, NewStream>,
}

impl PeerControl {
    // TOOD: Things that we could make configurable:
    // - Timeout for opening a new stream
    pub async fn open_stream(&mut self) -> Result<Stream, Error> {
        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(NewStream {
                protocol: self.protocol.clone(),
                sender,
            })
            .await
            .map_err(|e| Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, e)))?;

        let stream = receiver
            .await
            .map_err(|e| Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, e)))??;

        Ok(stream)
    }
}

#[derive(Debug)]
pub enum Error {
    /// The remote does not support the requested protocol.
    UnsupportedProtocol,
    /// IO Error that occurred during the protocol handshake.
    Io(std::io::Error),
}

pub struct IncomingStreams {
    receiver: mpsc::Receiver<(PeerId, Stream)>,
}

impl IncomingStreams {
    pub async fn next(&mut self) -> Option<(PeerId, Stream)> {
        self.receiver.next().await
    }

    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<(PeerId, Stream)>> {
        self.receiver.poll_next_unpin(cx)
    }
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
    ) -> Result<swarm::THandler<Self>, swarm::ConnectionDenied> {
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
    ) -> Result<swarm::THandler<Self>, swarm::ConnectionDenied> {
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

impl ConnectionHandler for Handler {
    type FromBehaviour = RegisterProtocol;
    type ToBehaviour = void::Void;
    type InboundProtocol = Upgrade;
    type OutboundProtocol = Upgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(
        &self,
    ) -> swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        swarm::SubstreamProtocol::new(
            Upgrade {
                supported_protocols: self.supported_protocols.keys().cloned().collect(),
            },
            (),
        )
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        if self.pending_upgrade.is_some() {
            return Poll::Pending;
        }

        match self.receiver.poll_next_unpin(cx) {
            Poll::Ready(Some(new_stream)) => {
                self.pending_upgrade = Some((new_stream.protocol.clone(), new_stream.sender));
                Poll::Ready(swarm::ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: swarm::SubstreamProtocol::new(
                        Upgrade {
                            supported_protocols: vec![new_stream.protocol],
                        },
                        (),
                    ),
                })
            }
            Poll::Ready(None) => {
                // Sender is gone, no more work to do.
                Poll::Pending
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        self.supported_protocols
            .insert(event.protocol, event.sender);
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: (stream, protocol),
                info: (),
            }) => {
                match self.supported_protocols.get_mut(&protocol) {
                    Some(sender) => match sender.try_send((self.remote, stream)) {
                        Ok(()) => {}
                        Err(e) if e.is_full() => {
                            tracing::debug!(%protocol, "channel is full, dropping inbound stream");
                        }
                        Err(e) if e.is_disconnected() => {
                            // TODO: Remove the sender from the hashmap here?
                            tracing::debug!(%protocol, "channel is gone, dropping inbound stream");
                        }
                        _ => unreachable!(),
                    },
                    None => {
                        // TODO: Can this be hit if the hashmap gets updated whilst we are negotiating?
                        debug_assert!(
                            false,
                            "Negotiated an inbound stream for {protocol} without having a sender"
                        );
                    }
                }
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: (stream, actual_protocol),
                info: (),
            }) => {
                let Some((expected_protocol, sender)) = self.pending_upgrade.take() else {
                    debug_assert!(
                        false,
                        "Negotiated an outbound stream without a back channel"
                    );
                    return;
                };
                debug_assert_eq!(expected_protocol, actual_protocol);

                let _ = sender.send(Ok(stream));
            }
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, info: () }) => {
                let Some((_, sender)) = self.pending_upgrade.take() else {
                    debug_assert!(
                        false,
                        "Received a `DialUpgradeError` without a back channel"
                    );
                    return;
                };

                let error = match error {
                    swarm::StreamUpgradeError::Timeout => {
                        Error::Io(io::Error::from(io::ErrorKind::TimedOut))
                    }
                    swarm::StreamUpgradeError::Apply(v) => void::unreachable(v),
                    swarm::StreamUpgradeError::NegotiationFailed => Error::UnsupportedProtocol,
                    swarm::StreamUpgradeError::Io(io) => Error::Io(io),
                };

                let _ = sender.send(Err(error));
            }
            _ => {}
        }
    }
}

pub struct Upgrade {
    supported_protocols: Vec<StreamProtocol>,
}

impl UpgradeInfo for Upgrade {
    type Info = StreamProtocol;

    type InfoIter = std::vec::IntoIter<StreamProtocol>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.supported_protocols.clone().into_iter()
    }
}

impl InboundUpgrade<Stream> for Upgrade {
    type Output = (Stream, StreamProtocol);

    type Error = void::Void;

    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: Stream, info: Self::Info) -> Self::Future {
        ready(Ok((socket, info)))
    }
}

impl OutboundUpgrade<Stream> for Upgrade {
    type Output = (Stream, StreamProtocol);

    type Error = void::Void;

    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: Stream, info: Self::Info) -> Self::Future {
        ready(Ok((socket, info)))
    }
}
