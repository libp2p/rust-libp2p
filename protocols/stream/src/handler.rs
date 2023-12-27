use std::{
    collections::{hash_map::Entry, HashMap},
    io,
    task::{Context, Poll},
};

use flume::{
    r#async::{RecvStream, SendSink},
    Receiver,
};
use futures::{channel::oneshot, SinkExt, StreamExt as _};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    self as swarm,
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound},
    ConnectionHandler, Stream, StreamProtocol,
};

use crate::{upgrade::Upgrade, OpenStreamError};

pub struct Handler {
    remote: PeerId,
    supported_protocols: HashMap<StreamProtocol, SendSink<'static, (PeerId, Stream)>>,

    receiver: Receiver<NewStream>,
    receive_stream: RecvStream<'static, NewStream>,
    pending_upgrade: Option<(
        StreamProtocol,
        oneshot::Sender<Result<Stream, OpenStreamError>>,
    )>,
}

impl Handler {
    pub(crate) fn new(
        remote: PeerId,
        supported_protocols: HashMap<StreamProtocol, SendSink<'static, (PeerId, Stream)>>,
        receiver: Receiver<NewStream>,
    ) -> Self {
        Self {
            supported_protocols,
            receive_stream: receiver.clone().into_stream(),
            receiver,
            pending_upgrade: None,
            remote,
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

        match self.receive_stream.poll_next_unpin(cx) {
            Poll::Ready(Some(new_stream)) => {
                self.pending_upgrade = Some((new_stream.protocol.clone(), new_stream.sender));
                return Poll::Ready(swarm::ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: swarm::SubstreamProtocol::new(
                        Upgrade {
                            supported_protocols: vec![new_stream.protocol],
                        },
                        (),
                    ),
                });
            }
            Poll::Ready(None) => {} // Sender is gone, no more work to do.
            Poll::Pending => {}
        }

        Poll::Pending
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
            }) => match self.supported_protocols.entry(protocol.clone()) {
                Entry::Occupied(mut entry) => {
                    match entry.get_mut().sender().try_send((self.remote, stream)) {
                        Ok(()) => {}
                        Err(flume::TrySendError::Full(_)) => {
                            tracing::debug!(%protocol, "channel is full, dropping inbound stream");
                        }
                        Err(flume::TrySendError::Disconnected(_)) => {
                            tracing::debug!(%protocol, "channel is gone, dropping inbound stream");
                            entry.remove();
                        }
                    }
                }
                Entry::Vacant(_) => {
                    tracing::debug!(%protocol, "channel is gone, dropping inbound stream");
                }
            },
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
                        OpenStreamError::Io(io::Error::from(io::ErrorKind::TimedOut))
                    }
                    swarm::StreamUpgradeError::Apply(v) => void::unreachable(v),
                    swarm::StreamUpgradeError::NegotiationFailed => {
                        OpenStreamError::UnsupportedProtocol
                    }
                    swarm::StreamUpgradeError::Io(io) => OpenStreamError::Io(io),
                };

                let _ = sender.send(Err(error));
            }
            _ => {}
        }
    }

    fn connection_keep_alive(&self) -> bool {
        tracing::debug!(sender_count = %self.receiver.sender_count());

        let any_peer_controls_alive = self.receiver.sender_count() > 1; // The behaviour always owns a copy of the sender.

        any_peer_controls_alive
    }
}

/// Message from a [`PeerControl`] to a [`ConnectionHandler`] to negotiate a new outbound stream.
pub(crate) struct NewStream {
    pub(crate) protocol: StreamProtocol,
    pub(crate) sender: oneshot::Sender<Result<Stream, OpenStreamError>>,
}

pub(crate) enum ToHandler {
    RegisterProtocol(RegisterProtocol),
}

pub struct RegisterProtocol {
    pub(crate) protocol: StreamProtocol,
    pub(crate) sender: SendSink<'static, (PeerId, Stream)>,
}

impl std::fmt::Debug for RegisterProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterProtocol")
            .field("protocol", &self.protocol)
            .finish()
    }
}
