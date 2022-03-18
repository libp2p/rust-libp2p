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

use crate::config::ValidationMode;
use crate::error::{GossipsubHandlerError, ValidationError};
use crate::protocol::{GossipsubCodec, ProtocolConfig};
use crate::types::{GossipsubRpc, PeerKind, RawGossipsubMessage};
use asynchronous_codec::Framed;
use futures::prelude::*;
use futures::StreamExt;
use instant::Instant;
use libp2p_core::upgrade::{InboundUpgrade, NegotiationError, OutboundUpgrade, UpgradeError};
use libp2p_swarm::handler::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    SubstreamProtocol,
};
use libp2p_swarm::NegotiatedSubstream;
use log::{error, trace, warn};
use smallvec::SmallVec;
use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

/// The initial time (in seconds) we set the keep alive for protocol negotiations to occur.
const INITIAL_KEEP_ALIVE: u64 = 30;

/// The event emitted by the Handler. This informs the behaviour of various events created
/// by the handler.
#[derive(Debug)]
pub enum HandlerEvent {
    /// A GossipsubRPC message has been received. This also contains a list of invalid messages (if
    /// any) that were received.
    Message {
        /// The GossipsubRPC message excluding any invalid messages.
        rpc: GossipsubRpc,
        /// Any invalid messages that were received in the RPC, along with the associated
        /// validation error.
        invalid_messages: Vec<(RawGossipsubMessage, ValidationError)>,
    },
    /// An inbound or outbound substream has been established with the peer and this informs over
    /// which protocol. This message only occurs once per connection.
    PeerKind(PeerKind),
}

/// A message sent from the behaviour to the handler.
#[derive(Debug, Clone)]
pub enum GossipsubHandlerIn {
    /// A gossipsub message to send.
    Message(crate::rpc_proto::Rpc),
    /// The peer has joined the mesh.
    JoinedMesh,
    /// The peer has left the mesh.
    LeftMesh,
}

/// The maximum number of substreams we accept or create before disconnecting from the peer.
///
/// Gossipsub is supposed to have a single long-lived inbound and outbound substream. On failure we
/// attempt to recreate these. This imposes an upper bound of new substreams before we consider the
/// connection faulty and disconnect. This also prevents against potential substream creation loops.
const MAX_SUBSTREAM_CREATION: usize = 5;

/// Protocol Handler that manages a single long-lived substream with a peer.
pub struct GossipsubHandler {
    /// Upgrade configuration for the gossipsub protocol.
    listen_protocol: SubstreamProtocol<ProtocolConfig, ()>,

    /// The single long-lived outbound substream.
    outbound_substream: Option<OutboundSubstreamState>,

    /// The single long-lived inbound substream.
    inbound_substream: Option<InboundSubstreamState>,

    /// Queue of values that we want to send to the remote.
    send_queue: SmallVec<[crate::rpc_proto::Rpc; 16]>,

    /// Flag indicating that an outbound substream is being established to prevent duplicate
    /// requests.
    outbound_substream_establishing: bool,

    /// The number of outbound substreams we have created.
    outbound_substreams_created: usize,

    /// The number of inbound substreams that have been created by the peer.
    inbound_substreams_created: usize,

    /// The type of peer this handler is associated to.
    peer_kind: Option<PeerKind>,

    /// Keeps track on whether we have sent the peer kind to the behaviour.
    //
    // NOTE: Use this flag rather than checking the substream count each poll.
    peer_kind_sent: bool,

    /// If the peer doesn't support the gossipsub protocol we do not immediately disconnect.
    /// Rather, we disable the handler and prevent any incoming or outgoing substreams from being
    /// established.
    ///
    /// This value is set to true to indicate the peer doesn't support gossipsub.
    protocol_unsupported: bool,

    /// The amount of time we allow idle connections before disconnecting.
    idle_timeout: Duration,

    /// Collection of errors from attempting an upgrade.
    upgrade_errors: VecDeque<ConnectionHandlerUpgrErr<GossipsubHandlerError>>,

    /// Flag determining whether to maintain the connection to the peer.
    keep_alive: KeepAlive,

    /// Keeps track of whether this connection is for a peer in the mesh. This is used to make
    /// decisions about the keep alive state for this connection.
    in_mesh: bool,
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
    PendingSend(
        Framed<NegotiatedSubstream, GossipsubCodec>,
        crate::rpc_proto::Rpc,
    ),
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(Framed<NegotiatedSubstream, GossipsubCodec>),
    /// The substream is being closed. Used by either substream.
    _Closing(Framed<NegotiatedSubstream, GossipsubCodec>),
    /// An error occurred during processing.
    Poisoned,
}

impl GossipsubHandler {
    /// Builds a new [`GossipsubHandler`].
    pub fn new(
        protocol_id_prefix: std::borrow::Cow<'static, str>,
        max_transmit_size: usize,
        validation_mode: ValidationMode,
        idle_timeout: Duration,
        support_floodsub: bool,
    ) -> Self {
        GossipsubHandler {
            listen_protocol: SubstreamProtocol::new(
                ProtocolConfig::new(
                    protocol_id_prefix,
                    max_transmit_size,
                    validation_mode,
                    support_floodsub,
                ),
                (),
            ),
            inbound_substream: None,
            outbound_substream: None,
            outbound_substream_establishing: false,
            outbound_substreams_created: 0,
            inbound_substreams_created: 0,
            send_queue: SmallVec::new(),
            peer_kind: None,
            peer_kind_sent: false,
            protocol_unsupported: false,
            idle_timeout,
            upgrade_errors: VecDeque::new(),
            keep_alive: KeepAlive::Until(Instant::now() + Duration::from_secs(INITIAL_KEEP_ALIVE)),
            in_mesh: false,
        }
    }
}

impl ConnectionHandler for GossipsubHandler {
    type InEvent = GossipsubHandlerIn;
    type OutEvent = HandlerEvent;
    type Error = GossipsubHandlerError;
    type InboundOpenInfo = ();
    type InboundProtocol = ProtocolConfig;
    type OutboundOpenInfo = crate::rpc_proto::Rpc;
    type OutboundProtocol = ProtocolConfig;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        self.listen_protocol.clone()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _info: Self::InboundOpenInfo,
    ) {
        let (substream, peer_kind) = protocol;

        // If the peer doesn't support the protocol, reject all substreams
        if self.protocol_unsupported {
            return;
        }

        self.inbound_substreams_created += 1;

        // update the known kind of peer
        if self.peer_kind.is_none() {
            self.peer_kind = Some(peer_kind);
        }

        // new inbound substream. Replace the current one, if it exists.
        trace!("New inbound substream request");
        self.inbound_substream = Some(InboundSubstreamState::WaitingInput(substream));
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        message: Self::OutboundOpenInfo,
    ) {
        let (substream, peer_kind) = protocol;

        // If the peer doesn't support the protocol, reject all substreams
        if self.protocol_unsupported {
            return;
        }

        self.outbound_substream_establishing = false;
        self.outbound_substreams_created += 1;

        // update the known kind of peer
        if self.peer_kind.is_none() {
            self.peer_kind = Some(peer_kind);
        }

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

    fn inject_event(&mut self, message: GossipsubHandlerIn) {
        if !self.protocol_unsupported {
            match message {
                GossipsubHandlerIn::Message(m) => self.send_queue.push(m),
                // If we have joined the mesh, keep the connection alive.
                GossipsubHandlerIn::JoinedMesh => {
                    self.in_mesh = true;
                    self.keep_alive = KeepAlive::Yes;
                }
                // If we have left the mesh, start the idle timer.
                GossipsubHandlerIn::LeftMesh => {
                    self.in_mesh = false;
                    self.keep_alive = KeepAlive::Until(Instant::now() + self.idle_timeout);
                }
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        e: ConnectionHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
        self.outbound_substream_establishing = false;
        warn!("Dial upgrade error {:?}", e);
        self.upgrade_errors.push_back(e);
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        // Handle any upgrade errors
        if let Some(error) = self.upgrade_errors.pop_front() {
            let reported_error = match error {
                // Timeout errors get mapped to NegotiationTimeout and we close the connection.
                ConnectionHandlerUpgrErr::Timeout | ConnectionHandlerUpgrErr::Timer => {
                    Some(GossipsubHandlerError::NegotiationTimeout)
                }
                // There was an error post negotiation, close the connection.
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Apply(e)) => Some(e),
                ConnectionHandlerUpgrErr::Upgrade(UpgradeError::Select(negotiation_error)) => {
                    match negotiation_error {
                        NegotiationError::Failed => {
                            // The protocol is not supported
                            self.protocol_unsupported = true;
                            if !self.peer_kind_sent {
                                self.peer_kind_sent = true;
                                // clear all substreams so the keep alive returns false
                                self.inbound_substream = None;
                                self.outbound_substream = None;
                                self.keep_alive = KeepAlive::No;
                                return Poll::Ready(ConnectionHandlerEvent::Custom(
                                    HandlerEvent::PeerKind(PeerKind::NotSupported),
                                ));
                            } else {
                                None
                            }
                        }
                        NegotiationError::ProtocolError(e) => {
                            Some(GossipsubHandlerError::NegotiationProtocolError(e))
                        }
                    }
                }
            };

            // If there was a fatal error, close the connection.
            if let Some(error) = reported_error {
                return Poll::Ready(ConnectionHandlerEvent::Close(error));
            }
        }

        if !self.peer_kind_sent {
            if let Some(peer_kind) = self.peer_kind.as_ref() {
                self.peer_kind_sent = true;
                return Poll::Ready(ConnectionHandlerEvent::Custom(HandlerEvent::PeerKind(
                    peer_kind.clone(),
                )));
            }
        }

        if self.inbound_substreams_created > MAX_SUBSTREAM_CREATION {
            // Too many inbound substreams have been created, end the connection.
            return Poll::Ready(ConnectionHandlerEvent::Close(
                GossipsubHandlerError::MaxInboundSubstreams,
            ));
        }

        // determine if we need to create the stream
        if !self.send_queue.is_empty()
            && self.outbound_substream.is_none()
            && !self.outbound_substream_establishing
        {
            if self.outbound_substreams_created >= MAX_SUBSTREAM_CREATION {
                return Poll::Ready(ConnectionHandlerEvent::Close(
                    GossipsubHandlerError::MaxOutboundSubstreams,
                ));
            }
            let message = self.send_queue.remove(0);
            self.send_queue.shrink_to_fit();
            self.outbound_substream_establishing = true;
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: self.listen_protocol.clone().map_info(|()| message),
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
                            if !self.in_mesh {
                                self.keep_alive =
                                    KeepAlive::Until(Instant::now() + self.idle_timeout);
                            }
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            return Poll::Ready(ConnectionHandlerEvent::Custom(message));
                        }
                        Poll::Ready(Some(Err(error))) => {
                            match error {
                                GossipsubHandlerError::MaxTransmissionSize => {
                                    warn!("Message exceeded the maximum transmission size");
                                    self.inbound_substream =
                                        Some(InboundSubstreamState::WaitingInput(substream));
                                }
                                _ => {
                                    warn!("Inbound stream error: {}", error);
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
                            warn!("Peer closed their outbound stream");
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
                                warn!("Inbound substream error while closing: {:?}", e);
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

        // process outbound stream
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
                                Err(GossipsubHandlerError::MaxTransmissionSize) => {
                                    error!("Message exceeded the maximum transmission size and was not sent.");
                                    self.outbound_substream =
                                        Some(OutboundSubstreamState::WaitingOutput(substream));
                                }
                                Err(e) => {
                                    error!("Error sending message: {}", e);
                                    return Poll::Ready(ConnectionHandlerEvent::Close(e));
                                }
                            }
                        }
                        Poll::Ready(Err(e)) => {
                            error!("Outbound substream error while sending output: {:?}", e);
                            return Poll::Ready(ConnectionHandlerEvent::Close(e));
                        }
                        Poll::Pending => {
                            self.keep_alive = KeepAlive::Yes;
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingSend(substream, message));
                            break;
                        }
                    }
                }
                Some(OutboundSubstreamState::PendingFlush(mut substream)) => {
                    match Sink::poll_flush(Pin::new(&mut substream), cx) {
                        Poll::Ready(Ok(())) => {
                            if !self.in_mesh {
                                // if not in the mesh, reset the idle timeout
                                self.keep_alive =
                                    KeepAlive::Until(Instant::now() + self.idle_timeout);
                            }
                            self.outbound_substream =
                                Some(OutboundSubstreamState::WaitingOutput(substream))
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(ConnectionHandlerEvent::Close(e))
                        }
                        Poll::Pending => {
                            self.keep_alive = KeepAlive::Yes;
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
                            warn!("Outbound substream error while closing: {:?}", e);
                            return Poll::Ready(ConnectionHandlerEvent::Close(
                                io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    "Failed to close outbound substream",
                                )
                                .into(),
                            ));
                        }
                        Poll::Pending => {
                            self.keep_alive = KeepAlive::No;
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
