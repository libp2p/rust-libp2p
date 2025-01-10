use std::{
    convert::Infallible,
    io,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use futures::{
    channel::{mpsc, oneshot},
    StreamExt as _,
};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    self as swarm,
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound},
    ConnectionHandler, Stream, StreamProtocol,
};

use crate::{shared::Shared, upgrade::Upgrade, OpenStreamError};

pub struct Handler {
    remote: PeerId,
    shared: Arc<Mutex<Shared>>,

    receiver: mpsc::Receiver<NewStream>,
    pending_upgrade: Option<(
        StreamProtocol,
        oneshot::Sender<Result<Stream, OpenStreamError>>,
    )>,
}

impl Handler {
    pub(crate) fn new(
        remote: PeerId,
        shared: Arc<Mutex<Shared>>,
        receiver: mpsc::Receiver<NewStream>,
    ) -> Self {
        Self {
            shared,
            receiver,
            pending_upgrade: None,
            remote,
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Infallible;
    type ToBehaviour = Infallible;
    type InboundProtocol = Upgrade;
    type OutboundProtocol = Upgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> swarm::SubstreamProtocol<Self::InboundProtocol> {
        swarm::SubstreamProtocol::new(
            Upgrade {
                supported_protocols: Shared::lock(&self.shared).supported_inbound_protocols(),
            },
            (),
        )
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<swarm::ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        if self.pending_upgrade.is_some() {
            return Poll::Pending;
        }

        match self.receiver.poll_next_unpin(cx) {
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
        // TODO: remove when Rust 1.82 is MSRV
        #[allow(unreachable_patterns)]
        libp2p_core::util::unreachable(event)
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: (stream, protocol),
                info: (),
            }) => {
                Shared::lock(&self.shared).on_inbound_stream(self.remote, stream, protocol);
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
                let Some((p, sender)) = self.pending_upgrade.take() else {
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
                    // TODO: remove when Rust 1.82 is MSRV
                    #[allow(unreachable_patterns)]
                    swarm::StreamUpgradeError::Apply(v) => libp2p_core::util::unreachable(v),
                    swarm::StreamUpgradeError::NegotiationFailed => {
                        OpenStreamError::UnsupportedProtocol(p)
                    }
                    swarm::StreamUpgradeError::Io(io) => OpenStreamError::Io(io),
                };

                let _ = sender.send(Err(error));
            }
            _ => {}
        }
    }
}

/// Message from a [`Control`](crate::Control) to
/// a [`ConnectionHandler`] to negotiate a new outbound stream.
#[derive(Debug)]
pub(crate) struct NewStream {
    pub(crate) protocol: StreamProtocol,
    pub(crate) sender: oneshot::Sender<Result<Stream, OpenStreamError>>,
}
