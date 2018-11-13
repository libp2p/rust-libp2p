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

use futures::prelude::*;
use libp2p_core::nodes::{NodeHandlerEndpoint, ProtocolsHandler, ProtocolsHandlerEvent};
use libp2p_core::ConnectionUpgrade;
use protocol::{FloodsubCodec, FloodsubConfig, FloodsubRpc};
use smallvec::SmallVec;
use std::{fmt, io};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

/// Protocol handler that handles communications with the remote for the fileshare protocol.
///
/// The handler will automatically open a substream with the remote for each request we make.
///
/// It also handles requests made by the remote.
pub struct FloodsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Configuration for the Kademlia protocol.
    config: FloodsubConfig,

    /// If true, we are trying to shut down the existing Kademlia substream and should refuse any
    /// incoming connection.
    shutting_down: bool,

    /// The active substreams.
    // TODO: add a limit to the number of allowed substreams
    substreams: Vec<SubstreamState<TSubstream>>,

    /// Queue of values that we want to send to the remote.
    send_queue: SmallVec<[FloodsubRpc; 16]>,
}

/// State of an active substream, opened either by us or by the remote.
enum SubstreamState<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Waiting for a message from the remote.
    WaitingInput(Framed<TSubstream, FloodsubCodec>),
    /// Waiting to send a message to the remote.
    PendingSend(Framed<TSubstream, FloodsubCodec>, FloodsubRpc),
    /// Waiting to send a message to the remote.
    /// Waiting to flush the substream so that the data arrives to the remote.
    PendingFlush(Framed<TSubstream, FloodsubCodec>),
    /// The substream is being closed.
    Closing(Framed<TSubstream, FloodsubCodec>),
}

impl<TSubstream> SubstreamState<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Consumes this state and produces the substream, if relevant.
    fn into_substream(self) -> Option<Framed<TSubstream, FloodsubCodec>> {
        match self {
            SubstreamState::WaitingInput(substream) => Some(substream),
            SubstreamState::PendingSend(substream, _) => Some(substream),
            SubstreamState::PendingFlush(substream) => Some(substream),
            SubstreamState::Closing(substream) => Some(substream),
        }
    }
}

impl<TSubstream> FloodsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Builds a new `FloodsubHandler`.
    pub fn new() -> Self {
        FloodsubHandler {
            config: FloodsubConfig::new(),
            shutting_down: false,
            substreams: Vec::new(),
            send_queue: SmallVec::new(),
        }
    }
}

impl<TSubstream> ProtocolsHandler for FloodsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + 'static,
{
    type InEvent = FloodsubRpc;
    type OutEvent = FloodsubRpc;
    type Substream = TSubstream;
    type Protocol = FloodsubConfig;
    type OutboundOpenInfo = FloodsubRpc;

    #[inline]
    fn listen_protocol(&self) -> Self::Protocol {
        self.config.clone()
    }

    fn inject_fully_negotiated(
        &mut self,
        protocol: <Self::Protocol as ConnectionUpgrade<TSubstream>>::Output,
        endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        if self.shutting_down {
            return;
        }

        match endpoint {
            NodeHandlerEndpoint::Dialer(message) => {
                self.substreams
                    .push(SubstreamState::PendingSend(protocol, message));
            }
            NodeHandlerEndpoint::Listener => {
                self.substreams.push(SubstreamState::WaitingInput(protocol));
            }
        }
    }

    #[inline]
    fn inject_event(&mut self, message: FloodsubRpc) {
        self.send_queue.push(message);
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: io::Error) {}

    #[inline]
    fn shutdown(&mut self) {
        self.shutting_down = true;
        for n in (0..self.substreams.len()).rev() {
            let mut substream = self.substreams.swap_remove(n);
            if let Some(substream) = substream.into_substream() {
                self.substreams.push(SubstreamState::Closing(substream));
            }
        }
    }

    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::Protocol, Self::OutboundOpenInfo, Self::OutEvent>>,
        io::Error,
    > {
        if !self.send_queue.is_empty() {
            let message = self.send_queue.remove(0);
            return Ok(Async::Ready(Some(
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    info: message,
                    upgrade: self.config.clone(),
                },
            )));
        }

        for n in (0..self.substreams.len()).rev() {
            let mut substream = self.substreams.swap_remove(n);
            loop {
                substream = match substream {
                    SubstreamState::WaitingInput(mut substream) => match substream.poll() {
                        Ok(Async::Ready(Some(message))) => {
                            self.substreams
                                .push(SubstreamState::WaitingInput(substream));
                            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(message))));
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
                        Err(_) => return Ok(Async::Ready(None)),
                    },
                }
            }
        }

        Ok(Async::NotReady)
    }
}

impl<TSubstream> fmt::Debug for FloodsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("FloodsubHandler")
            .field("shutting_down", &self.shutting_down)
            .field("substreams", &self.substreams.len())
            .field("send_queue", &self.send_queue.len())
            .finish()
    }
}
