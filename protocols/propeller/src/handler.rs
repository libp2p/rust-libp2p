//! Connection handler for the Propeller protocol.

use std::{
    collections::VecDeque,
    fmt,
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
use tracing::{debug, trace};

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

    /// Queue of events to be emitted to the behaviour.
    pending_events: VecDeque<HandlerOut>,

    /// Active inbound substream (max one at a time).
    inbound_substream: Option<Framed<Stream, PropellerCodec>>,

    /// Active outbound substream (max one at a time).
    outbound_substream: Option<OutboundState>,

    /// Whether we have requested an outbound substream.
    outbound_requested: bool,

    /// Timeout for substream upgrades.
    substream_timeout: Duration,
}

/// State of an outbound substream.
struct OutboundState {
    /// The framed substream.
    stream: Framed<Stream, PropellerCodec>,
    /// The message to be sent.
    message: PropellerMessage,
    /// Whether the message has been sent.
    sent: bool,
}

impl Handler {
    /// Create a new handler.
    pub fn new(
        protocol_id: libp2p_swarm::StreamProtocol,
        max_message_size: usize,
        substream_timeout: Duration,
    ) -> Self {
        Self {
            protocol: PropellerProtocol::new(protocol_id, max_message_size),
            pending_outbound: VecDeque::new(),
            pending_events: VecDeque::new(),
            inbound_substream: None,
            outbound_substream: None,
            outbound_requested: false,
            substream_timeout,
        }
    }

    /// Handle a fully negotiated inbound substream.
    fn on_fully_negotiated_inbound(&mut self, stream: Framed<Stream, PropellerCodec>) {
        if self.inbound_substream.is_some() {
            debug!("Dropping inbound substream: already have an active one");
            return;
        }

        debug!("New inbound substream established");
        self.inbound_substream = Some(stream);
    }

    /// Handle a fully negotiated outbound substream.
    fn on_fully_negotiated_outbound(&mut self, stream: Framed<Stream, PropellerCodec>) {
        self.outbound_requested = false;

        if let Some(message) = self.pending_outbound.pop_front() {
            debug!("New outbound substream established, sending message");
            self.outbound_substream = Some(OutboundState {
                stream,
                message,
                sent: false,
            });
        } else {
            debug!("Outbound substream established but no pending messages");
            // Close the stream since we have nothing to send
            drop(stream);
        }
    }

    /// Poll inbound substream for incoming messages.
    fn poll_inbound_substream(&mut self, cx: &mut Context<'_>) -> Poll<Option<HandlerOut>> {
        if let Some(stream) = &mut self.inbound_substream {
            match stream.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(message))) => {
                    trace!("Received message from inbound substream");
                    // Close the substream after receiving a message
                    self.inbound_substream = None;
                    return Poll::Ready(Some(HandlerOut::Message(message)));
                }
                Poll::Ready(Some(Err(e))) => {
                    debug!("Inbound substream error: {}", e);
                    self.inbound_substream = None;
                }
                Poll::Ready(None) => {
                    debug!("Inbound substream closed");
                    self.inbound_substream = None;
                }
                Poll::Pending => {}
            }
        }
        Poll::Pending
    }

    /// Poll outbound substream for message sending.
    fn poll_outbound_substream(&mut self, cx: &mut Context<'_>) -> Poll<Option<HandlerOut>> {
        if let Some(state) = &mut self.outbound_substream {
            if !state.sent {
                // Try to send the message
                match state.stream.poll_ready_unpin(cx) {
                    Poll::Ready(Ok(())) => {
                        match state.stream.start_send_unpin(state.message.clone()) {
                            Ok(()) => {
                                state.sent = true;
                                trace!("Message queued for sending on outbound substream");
                            }
                            Err(e) => {
                                debug!("Failed to queue message for sending: {}", e);
                                self.outbound_substream = None;
                                return Poll::Ready(Some(HandlerOut::SendError(format!(
                                    "Failed to queue message: {}",
                                    e
                                ))));
                            }
                        }
                    }
                    Poll::Ready(Err(e)) => {
                        debug!("Outbound substream not ready for sending: {}", e);
                        self.outbound_substream = None;
                        return Poll::Ready(Some(HandlerOut::SendError(format!(
                            "Substream not ready: {}",
                            e
                        ))));
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }

            // Try to flush the message
            match state.stream.poll_flush_unpin(cx) {
                Poll::Ready(Ok(())) => {
                    trace!("Message sent successfully on outbound substream");
                    // Message sent successfully, close the substream
                    self.outbound_substream = None;
                }
                Poll::Ready(Err(e)) => {
                    debug!("Failed to flush outbound substream: {}", e);
                    self.outbound_substream = None;
                    return Poll::Ready(Some(HandlerOut::SendError(format!(
                        "Failed to flush: {}",
                        e
                    ))));
                }
                Poll::Pending => {}
            }
        }
        Poll::Pending
    }

    /// Check if we need to request a new outbound substream.
    fn should_request_outbound_substream(&self) -> bool {
        !self.pending_outbound.is_empty()
            && self.outbound_substream.is_none()
            && !self.outbound_requested
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = HandlerIn;
    type ToBehaviour = HandlerOut;
    type InboundProtocol = PropellerProtocol;
    type OutboundProtocol = PropellerProtocol;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(self.protocol.clone(), ()).with_timeout(self.substream_timeout)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        // First, emit any pending events
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        // Poll inbound substream for incoming messages
        if let Poll::Ready(Some(event)) = self.poll_inbound_substream(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        // Poll outbound substream for message sending
        if let Poll::Ready(Some(event)) = self.poll_outbound_substream(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        // Request new outbound substream if needed
        if self.should_request_outbound_substream() {
            self.outbound_requested = true;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(self.protocol.clone(), ())
                    .with_timeout(self.substream_timeout),
            });
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            HandlerIn::SendMessage(message) => {
                trace!("Queuing message for sending");
                self.pending_outbound.push_back(message);
            }
        }
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: stream,
                info: (),
            }) => {
                self.on_fully_negotiated_inbound(stream);
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: stream,
                info: (),
            }) => {
                self.on_fully_negotiated_outbound(stream);
            }
            ConnectionEvent::DialUpgradeError(DialUpgradeError { error, info: () }) => {
                debug!("Outbound substream upgrade failed: {:?}", error);
                self.outbound_requested = false;

                // If we have pending messages, emit an error for the first one
                if !self.pending_outbound.is_empty() {
                    self.pending_outbound.pop_front(); // Remove the message that failed
                    self.pending_events.push_back(HandlerOut::SendError(format!(
                        "Substream upgrade failed: {:?}",
                        error
                    )));
                }
            }
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { error, info: () }) => {
                debug!("Inbound substream upgrade failed: {:?}", error);
                // For inbound failures, we don't need to notify the behaviour
                // as it's not expecting a response
            }
            ConnectionEvent::AddressChange(_) | ConnectionEvent::LocalProtocolsChange(_) => {
                // These events don't require any action for the Propeller protocol
            }
            _ => {
                // Handle any future connection events
            }
        }
    }
}

impl fmt::Debug for Handler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handler")
            .field("pending_outbound", &self.pending_outbound.len())
            .field("pending_events", &self.pending_events.len())
            .field("has_inbound_substream", &self.inbound_substream.is_some())
            .field("has_outbound_substream", &self.outbound_substream.is_some())
            .field("outbound_requested", &self.outbound_requested)
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
        assert_eq!(handler.pending_events.len(), 0);
        assert!(handler.inbound_substream.is_none());
        assert!(handler.outbound_substream.is_none());
        assert!(!handler.outbound_requested);
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
                data: vec![1, 2, 3, 4],
                signature: vec![],
            },
        };

        handler.on_behaviour_event(HandlerIn::SendMessage(message));
        assert_eq!(handler.pending_outbound.len(), 1);
        assert!(handler.should_request_outbound_substream());
    }
}
