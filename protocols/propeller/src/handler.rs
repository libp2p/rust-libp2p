//! Connection handler for the Propeller protocol.

use std::{
    collections::VecDeque,
    fmt,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use asynchronous_codec::Framed;
use futures::prelude::*;
use libp2p_swarm::{
    handler::{
        ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent, DialUpgradeError,
        FullyNegotiatedInbound, FullyNegotiatedOutbound, ListenUpgradeError,
    },
    Stream, SubstreamProtocol,
};
use tracing::trace;

use crate::{
    message::PropellerMessage,
    protocol::{PropellerCodec, PropellerProtocol},
};

/// Events that the handler can send to the behaviour.
#[derive(Debug)]
pub enum HandlerOut {
    /// A message was received from the remote peer.
    Message(PropellerMessage),
    /// An error occurred while sending a message.
    SendError(String),
}

/// Events that the behaviour can send to the handler.
#[derive(Debug, Clone)]
pub enum HandlerIn {
    /// Send a message to the remote peer.
    SendMessage(PropellerMessage),
}

/// Connection handler for the Propeller protocol.
pub struct Handler {
    /// The protocol configuration.
    protocol: PropellerProtocol,

    /// Queue of outbound messages to be sent.
    pending_outbound: VecDeque<PropellerMessage>,

    /// The single long-lived outbound substream.
    outbound_substream: Option<OutboundSubstreamState>,

    /// The single long-lived inbound substream.
    inbound_substream: Option<InboundSubstreamState>,

    /// Flag indicating that an outbound substream is being established.
    outbound_substream_establishing: bool,

    /// Timeout for substream upgrades.
    substream_timeout: Duration,
}

/// State of the inbound substream, opened either by us or by the remote.
enum InboundSubstreamState {
    /// Waiting for a message from the remote. The idle state for an inbound substream.
    WaitingInput(Framed<Stream, PropellerCodec>),
    /// The substream is being closed.
    Closing(Framed<Stream, PropellerCodec>),
    /// An error occurred during processing.
    Poisoned,
}

/// State of the outbound substream, opened either by us or by the remote.
enum OutboundSubstreamState {
    /// Waiting for the user to send a message. The idle state for an outbound substream.
    WaitingOutput(Framed<Stream, PropellerCodec>),
    /// Waiting to send a message to the remote.
    PendingSend(Framed<Stream, PropellerCodec>, PropellerMessage),
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(Framed<Stream, PropellerCodec>),
    /// An error occurred during processing.
    Poisoned,
}

impl Handler {
    /// Create a new handler.
    pub fn new(
        protocol_id: libp2p_swarm::StreamProtocol,
        max_shred_size: usize,
        substream_timeout: Duration,
    ) -> Self {
        Self {
            protocol: PropellerProtocol::new(protocol_id, max_shred_size),
            pending_outbound: VecDeque::new(),
            outbound_substream: None,
            inbound_substream: None,
            outbound_substream_establishing: false,
            substream_timeout,
        }
    }

    /// Handle a fully negotiated inbound substream.
    fn on_fully_negotiated_inbound(&mut self, stream: Framed<Stream, PropellerCodec>) {
        // new inbound substream. Replace the current one, if it exists.
        trace!("New inbound substream request");
        self.inbound_substream = Some(InboundSubstreamState::WaitingInput(stream));
    }

    /// Handle a fully negotiated outbound substream.
    fn on_fully_negotiated_outbound(&mut self, stream: Framed<Stream, PropellerCodec>) {
        assert!(
            self.outbound_substream.is_none(),
            "Established an outbound substream with one already available"
        );
        self.outbound_substream = Some(OutboundSubstreamState::WaitingOutput(stream));
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            <Handler as ConnectionHandler>::OutboundProtocol,
            (),
            <Handler as ConnectionHandler>::ToBehaviour,
        >,
    > {
        // determine if we need to create the outbound stream
        if !self.pending_outbound.is_empty()
            && self.outbound_substream.is_none()
            && !self.outbound_substream_establishing
        {
            self.outbound_substream_establishing = true;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(self.protocol.clone(), ()),
            });
        }

        // process outbound stream
        loop {
            match self
                .outbound_substream
                .replace(OutboundSubstreamState::Poisoned)
            {
                // outbound idle state
                Some(OutboundSubstreamState::WaitingOutput(substream)) => {
                    if let Some(message) = self.pending_outbound.pop_front() {
                        self.outbound_substream =
                            Some(OutboundSubstreamState::PendingSend(substream, message));
                        continue;
                    }

                    self.outbound_substream =
                        Some(OutboundSubstreamState::WaitingOutput(substream));
                    break;
                }
                Some(OutboundSubstreamState::PendingSend(mut substream, message)) => {
                    match Sink::poll_ready(Pin::new(&mut substream), cx) {
                        Poll::Ready(Ok(())) => {
                            match Sink::start_send(Pin::new(&mut substream), message) {
                                Ok(()) => {
                                    self.outbound_substream =
                                        Some(OutboundSubstreamState::PendingFlush(substream))
                                }
                                Err(e) => {
                                    tracing::debug!(
                                        "Failed to send message on outbound stream: {e}"
                                    );
                                    self.outbound_substream = None;
                                    break;
                                }
                            }
                        }
                        Poll::Ready(Err(e)) => {
                            tracing::debug!("Failed to send message on outbound stream: {e}");
                            self.outbound_substream = None;
                            break;
                        }
                        Poll::Pending => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingSend(substream, message));
                            break;
                        }
                    }
                }
                Some(OutboundSubstreamState::PendingFlush(mut substream)) => {
                    match Sink::poll_flush(Pin::new(&mut substream), cx) {
                        Poll::Ready(Ok(())) => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::WaitingOutput(substream))
                        }
                        Poll::Ready(Err(e)) => {
                            tracing::debug!("Failed to flush outbound stream: {e}");
                            self.outbound_substream = None;
                            break;
                        }
                        Poll::Pending => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingFlush(substream));
                            break;
                        }
                    }
                }
                None => {
                    self.outbound_substream = None;
                    break;
                }
                Some(OutboundSubstreamState::Poisoned) => {
                    unreachable!("Error occurred during outbound stream processing")
                }
            }
        }

        // Handle inbound messages.
        loop {
            match self
                .inbound_substream
                .replace(InboundSubstreamState::Poisoned)
            {
                // inbound idle state
                Some(InboundSubstreamState::WaitingInput(mut substream)) => {
                    match substream.poll_next_unpin(cx) {
                        Poll::Ready(Some(Ok(message))) => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                                HandlerOut::Message(message),
                            ));
                        }
                        Poll::Ready(Some(Err(error))) => {
                            tracing::debug!("Failed to read from inbound stream: {error}");
                            // Close this side of the stream. If the
                            // peer is still around, they will re-establish their
                            // outbound stream i.e. our inbound stream.
                            self.inbound_substream =
                                Some(InboundSubstreamState::Closing(substream));
                        }
                        // peer closed the stream
                        Poll::Ready(None) => {
                            tracing::debug!("Inbound stream closed by remote");
                            self.inbound_substream =
                                Some(InboundSubstreamState::Closing(substream));
                        }
                        Poll::Pending => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            break;
                        }
                    }
                }
                Some(InboundSubstreamState::Closing(mut substream)) => {
                    match Sink::poll_close(Pin::new(&mut substream), cx) {
                        Poll::Ready(res) => {
                            if let Err(e) = res {
                                // Don't close the connection but just drop the inbound substream.
                                // In case the remote has more to send, they will open up a new
                                // substream.
                                tracing::debug!("Inbound substream error while closing: {e}");
                            }
                            self.inbound_substream = None;
                            break;
                        }
                        Poll::Pending => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::Closing(substream));
                            break;
                        }
                    }
                }
                None => {
                    self.inbound_substream = None;
                    break;
                }
                Some(InboundSubstreamState::Poisoned) => {
                    unreachable!("Error occurred during inbound stream processing")
                }
            }
        }

        Poll::Pending
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = HandlerIn;
    type ToBehaviour = HandlerOut;
    type InboundProtocol = PropellerProtocol;
    type OutboundProtocol = PropellerProtocol;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(self.protocol.clone(), ()).with_timeout(self.substream_timeout)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        self.poll(cx)
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            HandlerIn::SendMessage(message) => {
                self.pending_outbound.push_back(message);
            }
        }
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
                protocol, ..
            }) => self.on_fully_negotiated_inbound(protocol),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol, ..
            }) => {
                self.outbound_substream_establishing = false;
                self.on_fully_negotiated_outbound(protocol)
            }
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, .. }) => {
                self.outbound_substream_establishing = false;
                tracing::debug!("Dial upgrade error: {:?}", error);
            }
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { error, .. }) => {
                tracing::debug!("Listen upgrade error: {:?}", error);
            }
            _ => {}
        }
    }
}

impl fmt::Debug for Handler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handler")
            .field("pending_outbound", &self.pending_outbound.len())
            .field(
                "outbound_substream",
                &self.outbound_substream.as_ref().map(|s| match s {
                    OutboundSubstreamState::WaitingOutput(_) => "WaitingOutput",
                    OutboundSubstreamState::PendingSend(_, _) => "PendingSend",
                    OutboundSubstreamState::PendingFlush(_) => "PendingFlush",
                    OutboundSubstreamState::Poisoned => "Poisoned",
                }),
            )
            .field(
                "inbound_substream",
                &self.inbound_substream.as_ref().map(|s| match s {
                    InboundSubstreamState::WaitingInput(_) => "WaitingInput",
                    InboundSubstreamState::Closing(_) => "Closing",
                    InboundSubstreamState::Poisoned => "Poisoned",
                }),
            )
            .field(
                "outbound_substream_establishing",
                &self.outbound_substream_establishing,
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use libp2p_swarm::StreamProtocol;

    use super::*;

    #[test]
    fn test_handler_creation() {
        let protocol_id = StreamProtocol::new("/propeller/1.0.0");
        let handler = Handler::new(protocol_id, 65536, Duration::from_secs(30));

        assert_eq!(handler.pending_outbound.len(), 0);
        assert!(handler.inbound_substream.is_none());
        assert!(handler.outbound_substream.is_none());
        assert!(!handler.outbound_substream_establishing);
    }

    #[test]
    fn test_handler_message_queuing() {
        let protocol_id = StreamProtocol::new("/propeller/1.0.0");
        let mut handler = Handler::new(protocol_id, 65536, Duration::from_secs(30));

        let message = PropellerMessage {
            shred: crate::message::Shred {
                id: crate::message::ShredId {
                    message_id: 1,
                    index: 0,
                    publisher: libp2p_identity::PeerId::random(),
                },
                shard: vec![1, 2, 3, 4],
                signature: vec![],
            },
        };

        handler.on_behaviour_event(HandlerIn::SendMessage(message));
        assert_eq!(handler.pending_outbound.len(), 1);
    }
}
