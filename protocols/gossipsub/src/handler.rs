// Copyright 2018 Parity Technologies (UK) Ltd.
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
use crate::protocol::{GossipsubCodec, ProtocolConfig};
use futures::prelude::*;
use libp2p_core::upgrade::{InboundUpgrade, Negotiated, OutboundUpgrade};
use libp2p_swarm::protocols_handler::{
    KeepAlive, ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use log::{trace, warn};
use smallvec::SmallVec;
use std::borrow::Cow;
use std::io;
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

/// Protocol Handler that manages a single long-lived substream with a peer.
pub struct GossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Upgrade configuration for the gossipsub protocol.
    listen_protocol: SubstreamProtocol<ProtocolConfig>,

    /// The single long-lived outbound substream.
    outbound_substream: Option<OutboundSubstreamState<TSubstream>>,

    /// The single long-lived inbound substream.
    inbound_substream: Option<InboundSubstreamState<TSubstream>>,

    /// Queue of values that we want to send to the remote.
    send_queue: SmallVec<[GossipsubRpc; 16]>,

    /// Flag determining whether to maintain the connection to the peer.
    keep_alive: KeepAlive,
}

/// State of the inbound substream, opened either by us or by the remote.
enum InboundSubstreamState<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Waiting for a message from the remote. The idle state for an inbound substream.
    WaitingInput(Framed<Negotiated<TSubstream>, GossipsubCodec>),
    /// The substream is being closed.
    Closing(Framed<Negotiated<TSubstream>, GossipsubCodec>),
    /// An error occurred during processing.
    Poisoned,
}

/// State of the outbound substream, opened either by us or by the remote.
enum OutboundSubstreamState<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Waiting for the user to send a message. The idle state for an outbound substream.
    WaitingOutput(Framed<Negotiated<TSubstream>, GossipsubCodec>),
    /// Waiting to send a message to the remote.
    PendingSend(Framed<Negotiated<TSubstream>, GossipsubCodec>, GossipsubRpc),
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(Framed<Negotiated<TSubstream>, GossipsubCodec>),
    /// The substream is being closed. Used by either substream.
    _Closing(Framed<Negotiated<TSubstream>, GossipsubCodec>),
    /// An error occurred during processing.
    Poisoned,
}

impl<TSubstream> GossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Builds a new `GossipsubHandler`.
    pub fn new(protocol_id: impl Into<Cow<'static, [u8]>>, max_transmit_size: usize) -> Self {
        GossipsubHandler {
            listen_protocol: SubstreamProtocol::new(ProtocolConfig::new(
                protocol_id,
                max_transmit_size,
            )),
            inbound_substream: None,
            outbound_substream: None,
            send_queue: SmallVec::new(),
            keep_alive: KeepAlive::Yes,
        }
    }
}

impl<TSubstream> Default for GossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    fn default() -> Self {
        GossipsubHandler {
            listen_protocol: SubstreamProtocol::new(ProtocolConfig::default()),
            inbound_substream: None,
            outbound_substream: None,
            send_queue: SmallVec::new(),
            keep_alive: KeepAlive::Yes,
        }
    }
}

impl<TSubstream> ProtocolsHandler for GossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = GossipsubRpc;
    type OutEvent = GossipsubRpc;
    type Error = io::Error;
    type Substream = TSubstream;
    type InboundProtocol = ProtocolConfig;
    type OutboundProtocol = ProtocolConfig;
    type OutboundOpenInfo = GossipsubRpc;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        self.listen_protocol.clone()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        substream: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output,
    ) {
        // new inbound substream. Replace the current one, if it exists.
        trace!("New inbound substream request");
        self.inbound_substream = Some(InboundSubstreamState::WaitingInput(substream));
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        substream: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        message: Self::OutboundOpenInfo,
    ) {
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
            <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error,
        >,
    ) {
        // Ignore upgrade errors for now.
        // If a peer doesn't support this protocol, this will just ignore them, but not disconnect
        // them.
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        io::Error,
    > {
        // determine if we need to create the stream
        if !self.send_queue.is_empty() && self.outbound_substream.is_none() {
            let message = self.send_queue.remove(0);
            self.send_queue.shrink_to_fit();
            return Ok(Async::Ready(
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol: self.listen_protocol.clone(),
                    info: message,
                },
            ));
        }

        loop {
            match std::mem::replace(
                &mut self.inbound_substream,
                Some(InboundSubstreamState::Poisoned),
            ) {
                // inbound idle state
                Some(InboundSubstreamState::WaitingInput(mut substream)) => {
                    match substream.poll() {
                        Ok(Async::Ready(Some(message))) => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(message)));
                        }
                        // peer closed the stream
                        Ok(Async::Ready(None)) => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::Closing(substream));
                        }
                        Ok(Async::NotReady) => {
                            self.inbound_substream =
                                Some(InboundSubstreamState::WaitingInput(substream));
                            break;
                        }
                        Err(_) => {
                            self.inbound_substream = Some(InboundSubstreamState::Closing(substream))
                        }
                    }
                }
                Some(InboundSubstreamState::Closing(mut substream)) => match substream.close() {
                    Ok(Async::Ready(())) => {
                        self.inbound_substream = None;
                        if self.outbound_substream.is_none() {
                            self.keep_alive = KeepAlive::No;
                        }
                        break;
                    }
                    Ok(Async::NotReady) => {
                        self.inbound_substream = Some(InboundSubstreamState::Closing(substream));
                        break;
                    }
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "Failed to close stream",
                        ))
                    }
                },
                None => {
                    self.inbound_substream = None;
                    break;
                }
                Some(InboundSubstreamState::Poisoned) => {
                    panic!("Error occurred during inbound stream processing")
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
                    match substream.start_send(message)? {
                        AsyncSink::Ready => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingFlush(substream))
                        }
                        AsyncSink::NotReady(message) => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingSend(substream, message));
                            break;
                        }
                    }
                }
                Some(OutboundSubstreamState::PendingFlush(mut substream)) => {
                    match substream.poll_complete()? {
                        Async::Ready(()) => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::WaitingOutput(substream))
                        }
                        Async::NotReady => {
                            self.outbound_substream =
                                Some(OutboundSubstreamState::PendingFlush(substream));
                            break;
                        }
                    }
                }
                // Currently never used - manual shutdown may implement this in the future
                Some(OutboundSubstreamState::_Closing(mut substream)) => match substream.close() {
                    Ok(Async::Ready(())) => {
                        self.outbound_substream = None;
                        if self.inbound_substream.is_none() {
                            self.keep_alive = KeepAlive::No;
                        }
                        break;
                    }
                    Ok(Async::NotReady) => {
                        self.outbound_substream = Some(OutboundSubstreamState::_Closing(substream));
                        break;
                    }
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "Failed to close outbound substream",
                        ))
                    }
                },
                None => {
                    self.outbound_substream = None;
                    break;
                }
                Some(OutboundSubstreamState::Poisoned) => {
                    panic!("Error occurred during outbound stream processing")
                }
            }
        }

        Ok(Async::NotReady)
    }
}
