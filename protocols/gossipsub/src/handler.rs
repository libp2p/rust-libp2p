// Copyright 2020 Sigma Prime Pty Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use crate::behaviour::GossipsubRpc;
use crate::config::ValidationMode;
use crate::protocol::{GossipsubCodec, ProtocolConfig};
use futures::prelude::*;
use futures_codec::Framed;
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade};
use libp2p_swarm::protocols_handler::{
    KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use libp2p_swarm::NegotiatedSubstream;
use log::{debug, error, trace, warn};
use smallvec::SmallVec;
use std::{
    borrow::Cow,
    io,
    pin::Pin,
    task::{Context, Poll},
};

/// Protocol Handler that manages a single long-lived substream with a peer.
pub struct GossipsubHandler {
    /// Upgrade configuration for the gossipsub protocol.
    listen_protocol: SubstreamProtocol<ProtocolConfig, ()>,

    /// The single long-lived outbound substream.
    outbound_substream: Option<OutboundSubstreamState>,

    /// The single long-lived inbound substream.
    inbound_substream: Option<InboundSubstreamState>,

    /// Queue of values that we want to send to the remote.
    send_queue: SmallVec<[GossipsubRpc; 16]>,

    /// Flag indicating that an outbound substream is being established to prevent duplicate
    /// requests.
    outbound_substream_establishing: bool,

    /// Flag determining whether to maintain the connection to the peer.
    keep_alive: KeepAlive,
}

/// State of the inbound substream, opened either by us or by the remote.
enum InboundSubstreamState {
    /// Waiting for a message from the remote. The idle state for an inbound substream.
    WaitingInput(Framed<NegotiatedSubstream, GossipsubCodec>),
    /// The substream is being closed.
    Closing(Framed<NegotiatedSubstream, GossipsubCodec>),
    /// An error occurred during processing.
    Poisoned,
}

/// State of the outbound substream, opened either by us or by the remote.
enum OutboundSubstreamState {
    /// Waiting for the user to send a message. The idle state for an outbound substream.
    WaitingOutput(Framed<NegotiatedSubstream, GossipsubCodec>),
    /// Waiting to send a message to the remote.
    PendingSend(Framed<NegotiatedSubstream, GossipsubCodec>, GossipsubRpc),
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(Framed<NegotiatedSubstream, GossipsubCodec>),
    /// The substream is being closed. Used by either substream.
    _Closing(Framed<NegotiatedSubstream, GossipsubCodec>),
    /// An error occurred during processing.
    Poisoned,
}

impl GossipsubHandler {
    /// Builds a new `GossipsubHandler`.
    pub fn new(
        protocol_id: impl Into<Cow<'static, [u8]>>,
        max_transmit_size: usize,
        validation_mode: ValidationMode,
    ) -> Self {
        GossipsubHandler {
            listen_protocol: SubstreamProtocol::new(ProtocolConfig::new(
                protocol_id,
                max_transmit_size,
                validation_mode,
            ), ()),
            inbound_substream: None,
            outbound_substream: None,
            outbound_substream_establishing: false,
            send_queue: SmallVec::new(),
            keep_alive: KeepAlive::Yes,
        }
    }
}

impl ProtocolsHandler for GossipsubHandler {
    type InEvent = GossipsubRpc;
    type OutEvent = GossipsubRpc;
    type Error = io::Error;
    type InboundProtocol = ProtocolConfig;
    type OutboundProtocol = ProtocolConfig;
    type OutboundOpenInfo = GossipsubRpc;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        self.listen_protocol.clone()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _info: Self::InboundOpenInfo
    ) {
        // new inbound substream. Replace the current one, if it exists.
        trace!("New inbound substream request");
        self.inbound_substream = Some(InboundSubstreamState::WaitingInput(substream));
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        message: Self::OutboundOpenInfo,
    ) {
        self.outbound_substream_establishing = false;
        // Should never establish a new outbound substream if one already exists.
        // If this happens, an outbound message is not sent.
        if self.outbound_substream.is_some() {
            warn!("Established an outbound substream with one already available");
            // Add the message back to the send queue
            self.send_queue.push(message);
        } else {
            self.outbound_substream = Some(OutboundSubstreamState::PendingSend(substream, message));
        }
    }

    fn inject_event(&mut self, message: GossipsubRpc) {
        self.send_queue.push(message);
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ProtocolsHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
        self.outbound_substream_establishing = false;
        // Ignore upgrade errors for now.
        // If a peer doesn't support this protocol, this will just ignore them, but not disconnect
        // them.
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        // determine if we need to create the stream
        if !self.send_queue.is_empty()
            && self.outbound_substream.is_none()
            && !self.outbound_substream_establishing
        {
            let message = self.send_queue.remove(0);
            self.send_queue.shrink_to_fit();
            self.outbound_substream_establishing = true;
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: self.listen_protocol.clone().map_info(|()| message)
            });
        }

        loop {
            match std::mem::replace(
                &mut self.inbound_substream,
                Some(InboundSubstreamState::Poisoned),
            ) {
                // inbound idle state
                Some(InboundSubstreamState::WaitingInput(mut substream)) => {
                    match substream.poll_next_unpin(cx) {
                        Poll::Ready(Some(Ok(message))) => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            return Poll::Ready(ProtocolsHandlerEvent::Custom(message));
                        }
                        Poll::Ready(Some(Err(e))) => {
                            match e.kind() {
                                std::io::ErrorKind::InvalidData => {
                                    // Invalid message, ignore it and reset to waiting
                                    warn!("Invalid message received. Error: {}", e);
                                    self.inbound_substream =
                                        Some(InboundSubstreamState::WaitingInput(substream));
                                }
                                _ => {
                                    // More serious errors, close this side of the stream. If the
                                    // peer is still around, they will re-establish their
                                    // connection
                                    self.inbound_substream =
                                        Some(InboundSubstreamState::Closing(substream));
                                }
                            }
                        }
                        // peer closed the stream
                        Poll::Ready(None) => {
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
                                debug!("Inbound substream error while closing: {:?}", e);
                            }

                            self.inbound_substream = None;
                            if self.outbound_substream.is_none() {
                                self.keep_alive = KeepAlive::No;
                            }
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

        loop {
            match std::mem::replace(
                &mut self.outbound_substream,
                Some(OutboundSubstreamState::Poisoned),
            ) {
                // outbound idle state
                Some(OutboundSubstreamState::WaitingOutput(substream)) => {
                    if !self.send_queue.is_empty() {
                        let message = self.send_queue.remove(0);
                        self.send_queue.shrink_to_fit();
                        self.outbound_substream =
                            Some(OutboundSubstreamState::PendingSend(substream, message));
                    } else {
                        self.outbound_substream =
                            Some(OutboundSubstreamState::WaitingOutput(substream));
                        break;
                    }
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
                                    if let io::ErrorKind::PermissionDenied = e.kind() {
                                        error!("Message over the maximum transmission limit was not sent.");
                                        self.outbound_substream =
                                            Some(OutboundSubstreamState::WaitingOutput(substream));
                                    } else {
                                        return Poll::Ready(ProtocolsHandlerEvent::Close(e));
                                    }
                                }
                            }
                        }
                        Poll::Ready(Err(e)) => {
                            debug!("Outbound substream error while sending output: {:?}", e);
                            return Poll::Ready(ProtocolsHandlerEvent::Close(e));
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
                        Poll::Ready(Err(e)) => return Poll::Ready(ProtocolsHandlerEvent::Close(e)),
                        Poll::Pending => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingFlush(substream));
                            break;
                        }
                    }
                }
                // Currently never used - manual shutdown may implement this in the future
                Some(OutboundSubstreamState::_Closing(mut substream)) => {
                    match Sink::poll_close(Pin::new(&mut substream), cx) {
                        Poll::Ready(Ok(())) => {
                            self.outbound_substream = None;
                            if self.inbound_substream.is_none() {
                                self.keep_alive = KeepAlive::No;
                            }
                            break;
                        }
                        Poll::Ready(Err(e)) => {
                            debug!("Outbound substream error while closing: {:?}", e);
                            return Poll::Ready(ProtocolsHandlerEvent::Close(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "Failed to close outbound substream",
                            )));
                        }
                        Poll::Pending => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::_Closing(substream));
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

        Poll::Pending
    }
}
