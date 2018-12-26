use protocol::{GossipsubConfig,GossipsubRpc,GossipsubCodec};
use futures::prelude::*;
use libp2p_core::{
    ProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerSelect,
    upgrade::{InboundUpgrade, OutboundUpgrade}
};
use libp2p_floodsub::{
    protocol::{FloodsubConfig, FloodsubRpc},
    handler::FloodsubHandler,
};
use smallvec::SmallVec;
use std::{fmt, io};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

/// Combines the `RawGossipsubHandler` and `FloodsubHandler` into one
/// protocol, to use as a handler for Gossipsub proper, which should be
/// backwards-compatible with Floodsub.
pub struct GossipsubHandler<RawGossipsubHandler,
    FloodsubHandler> {
    gossipsub: RawGossipsubHandler<TSubstream>,
    floodsub: FloodsubHandler<TSubstream>,
}

impl GossipsubHandler {
    fn new(gossipsub: RawGossipsubHandler<TSubstream>,
        floodsub: FloodsubHandler<TSubstream>) -> Self {
        ProtocolsHandlerSelect::new(
            gossipsub: RawGossipsubHandler<TSubstream>,
            floodsub: FloodsubHandler<TSubstream>
        );
    }
}

/// Protocol handler that handles communication with the remote for the
/// gossipsub protocol.
///
/// The handler will automatically open a substream with the remote for
/// each request we make.
///
/// It also handles requests made by the remote.
pub struct RawGossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Configuration for the Gossipsub protocol.
    config: GossipsubConfig,

    /// If true, we are trying to shut down the existing Gossipsub
    /// substream and should refuse any incoming connection.
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
    /// Waiting to flush the substream so that the data arrives to the
    /// remote.
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

impl<TSubstream> RawGossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Builds a new `GossipsubHandler`.
    pub fn new() -> Self {
        RawGossipsubHandler {
            config: GossipsubConfig::new(),
            shutting_down: false,
            substreams: Vec::new(),
            send_queue: SmallVec::new(),
        }
    }
}

impl<TSubstream> ProtocolsHandler for RawGossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = GossipsubRpc;
    type OutEvent = GossipsubRpc;
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
        protocol: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output
    ) {
        if self.shutting_down {
            return ()
        }
        self.substreams.push(SubstreamState::WaitingInput(protocol))
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        message: Self::OutboundOpenInfo
    ) {
        if self.shutting_down {
            return ()
        }
        self.substreams.push(SubstreamState::PendingSend(protocol, message))
    }

    #[inline]
    fn inject_event(&mut self, message: GossipsubRpc) {
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
            self.substreams.push(SubstreamState::Closing(substream.into_substream()));
        }
    }

    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>,
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

impl<TSubstream> fmt::Debug for RawGossipsubHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("RawGossipsubHandler")
            .field("shutting_down", &self.shutting_down)
            .field("substreams", &self.substreams.len())
            .field("send_queue", &self.send_queue.len())
            .finish()
    }
}
