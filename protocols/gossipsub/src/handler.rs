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

use crate::protocol::{GossipsubCodec, GossipsubConfig, GossipsubRpc};
use futures::prelude::*;
use libp2p_core::{
    protocols_handler::ProtocolsHandlerUpgrErr,
    upgrade::{InboundUpgrade, OutboundUpgrade},
    ProtocolsHandler, ProtocolsHandlerEvent,
};
use smallvec::SmallVec;
use std::{fmt, io};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

/// Protocol handler that handles communication with the remote for the gossipsub protocol.
///
/// The handler will automatically open a substream with the remote for each request we make.
///
/// It also handles requests made by the remote.
pub struct GossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Configuration for the gossipsub protocol.
    config: GossipsubConfig,

    /// If true, we are trying to shut down the existing gossipsub substream and should refuse any
    /// incoming connection.
    shutting_down: bool,

    /// The active substreams.
    // TODO: add a limit to the number of allowed substreams
    substreams: Vec<SubstreamState<TSubstream>>,

    /// Queue of values that we want to send to the remote.
    send_queue: SmallVec<[GossipsubRpc; 16]>,
}

/// State of an active substream, opened either by us or by the remote.
enum SubstreamState<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Waiting for a message from the remote.
    WaitingInput(Framed<TSubstream, GossipsubCodec>),
    /// Waiting to send a message to the remote.
    PendingSend(Framed<TSubstream, GossipsubCodec>, GossipsubRpc),
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(Framed<TSubstream, GossipsubCodec>),
    /// The substream is being closed.
    Closing(Framed<TSubstream, GossipsubCodec>),
}

impl<TSubstream> SubstreamState<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Consumes this state and produces the substream.
    fn into_substream(self) -> Framed<TSubstream, GossipsubCodec> {
        match self {
            SubstreamState::WaitingInput(substream) => substream,
            SubstreamState::PendingSend(substream, _) => substream,
            SubstreamState::PendingFlush(substream) => substream,
            SubstreamState::Closing(substream) => substream,
        }
    }
}

impl<TSubstream> GossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Builds a new `GossipsubHandler`.
    pub fn new() -> Self {
        GossipsubHandler {
            config: GossipsubConfig::new(),
            shutting_down: false,
            substreams: Vec::new(),
            send_queue: SmallVec::new(),
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
    type InboundProtocol = GossipsubConfig;
    type OutboundProtocol = GossipsubConfig;
    type OutboundOpenInfo = GossipsubRpc;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        self.config.clone()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output,
    ) {
        if self.shutting_down {
            return ();
        }
        self.substreams.push(SubstreamState::WaitingInput(protocol))
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        message: Self::OutboundOpenInfo,
    ) {
        if self.shutting_down {
            return ();
        }
        self.substreams
            .push(SubstreamState::PendingSend(protocol, message))
    }

    #[inline]
    fn inject_event(&mut self, message: GossipsubRpc) {
        self.send_queue.push(message);
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ProtocolsHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error,
        >,
    ) {
    }

    #[inline]
    fn connection_keep_alive(&self) -> bool {
        !self.substreams.is_empty()
    }

    #[inline]
    fn shutdown(&mut self) {
        self.shutting_down = true;
        for n in (0..self.substreams.len()).rev() {
            let mut substream = self.substreams.swap_remove(n);
            self.substreams
                .push(SubstreamState::Closing(substream.into_substream()));
        }
    }

    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        io::Error,
    > {
        if !self.send_queue.is_empty() {
            let message = self.send_queue.remove(0);
            return Ok(Async::Ready(
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    info: message,
                    upgrade: self.config.clone(),
                },
            ));
        }

        for n in (0..self.substreams.len()).rev() {
            let mut substream = self.substreams.swap_remove(n);
            loop {
                substream = match substream {
                    SubstreamState::WaitingInput(mut substream) => match substream.poll() {
                        Ok(Async::Ready(Some(message))) => {
                            self.substreams
                                .push(SubstreamState::WaitingInput(substream));
                            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(message)));
                        }
                        Ok(Async::Ready(None)) => SubstreamState::Closing(substream),
                        Ok(Async::NotReady) => {
                            self.substreams
                                .push(SubstreamState::WaitingInput(substream));
                            return Ok(Async::NotReady);
                        }
                        Err(_) => SubstreamState::Closing(substream),
                    },
                    SubstreamState::PendingSend(mut substream, message) => {
                        match substream.start_send(message)? {
                            AsyncSink::Ready => SubstreamState::PendingFlush(substream),
                            AsyncSink::NotReady(message) => {
                                self.substreams
                                    .push(SubstreamState::PendingSend(substream, message));
                                return Ok(Async::NotReady);
                            }
                        }
                    }
                    SubstreamState::PendingFlush(mut substream) => {
                        match substream.poll_complete()? {
                            Async::Ready(()) => SubstreamState::Closing(substream),
                            Async::NotReady => {
                                self.substreams
                                    .push(SubstreamState::PendingFlush(substream));
                                return Ok(Async::NotReady);
                            }
                        }
                    }
                    SubstreamState::Closing(mut substream) => match substream.close() {
                        Ok(Async::Ready(())) => break,
                        Ok(Async::NotReady) => {
                            self.substreams.push(SubstreamState::Closing(substream));
                            return Ok(Async::NotReady);
                        }
                        Err(_) => return Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown)),
                    },
                }
            }
        }

        Ok(Async::NotReady)
    }
}

impl<TSubstream> fmt::Debug for GossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("GossipsubHandler")
            .field("shutting_down", &self.shutting_down)
            .field("substreams", &self.substreams.len())
            .field("send_queue", &self.send_queue.len())
            .finish()
    }
}
