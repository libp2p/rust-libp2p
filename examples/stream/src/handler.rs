use std::{
    collections::HashMap,
    io,
    task::{Context, Poll},
};

use futures::{
    channel::{mpsc, oneshot},
    StreamExt as _,
};
use libp2p::{
    swarm::{
        self,
        handler::{
            ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        },
        ConnectionHandler,
    },
    PeerId, Stream, StreamProtocol,
};

use crate::{upgrade::Upgrade, Error};

pub struct Handler {
    remote: PeerId,
    supported_protocols: HashMap<StreamProtocol, mpsc::Sender<(PeerId, Stream)>>,

    receiver: flume::r#async::RecvStream<'static, NewStream>,
    pending_upgrade: Option<(StreamProtocol, oneshot::Sender<Result<Stream, Error>>)>,
}

impl Handler {
    pub(crate) fn new(
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

/// Message from a [`PeerControl`] to a [`ConnectionHandler`] to negotiate a new outbound stream.
pub(crate) struct NewStream {
    pub(crate) protocol: StreamProtocol,
    pub(crate) sender: oneshot::Sender<Result<Stream, Error>>,
}

pub(crate) enum ToHandler {
    RegisterProtocol(RegisterProtocol),
}

#[derive(Debug)]
pub struct RegisterProtocol {
    pub(crate) protocol: StreamProtocol,
    pub(crate) sender: mpsc::Sender<(PeerId, Stream)>,
}
