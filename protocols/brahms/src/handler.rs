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

use crate::protocol::{
    BrahmsListen, BrahmsListenOut, BrahmsListenPullRequest, BrahmsListenPullRequestFlush,
    BrahmsPullRequestRequest, BrahmsPushRequest,
};
use futures::prelude::*;
use libp2p_core::{
    either::{EitherError, EitherOutput},
    protocols_handler::ProtocolsHandlerUpgrErr,
    upgrade::{EitherUpgrade, InboundUpgrade, OutboundUpgrade},
    Multiaddr, PeerId, ProtocolsHandler, ProtocolsHandlerEvent,
};
use smallvec::SmallVec;
use std::{error, fmt, io};
use tokio_io::{AsyncRead, AsyncWrite};

pub struct BrahmsHandler<TSubstream> {
    /// If true, we are trying to shut down the existing floodsub substream and should refuse any
    /// incoming connection.
    shutting_down: bool,

    /// Queue of values that `poll` should produces.
    send_queue: SmallVec<
        [ProtocolsHandlerEvent<
            EitherUpgrade<BrahmsPushRequest, BrahmsPullRequestRequest>,
            (),
            BrahmsHandlerEvent,
        >; 16],
    >,

    /// Whether or not we should force the connection alive.
    connection_keep_alive: bool,

    /// The latest pull request received by the remote.
    ongoing_pull_request: Option<BrahmsListenPullRequest<TSubstream>>,

    /// Futures that flush pull responses.
    pull_response_flushes: SmallVec<[BrahmsListenPullRequestFlush<TSubstream>; 4]>,
}

#[derive(Debug, Clone)]
pub enum BrahmsHandlerIn {
    /// Marks this connection as "keep alive". We will not disconnect.
    EnableKeepAlive,
    /// Unmarks this connection from the "keep alive" state. We can disconnect.
    DisableKeepAlive,
    /// Send something to the remote.
    Event(BrahmsHandlerEvent),
}

#[derive(Debug, Clone)]
pub enum BrahmsHandlerEvent {
    Push { addresses: Vec<Multiaddr> },
    PullRequest,
    PullResult { list: Vec<(PeerId, Vec<Multiaddr>)> },
}

impl<TSubstream> BrahmsHandler<TSubstream> {
    /// Builds a new `BrahmsHandler`.
    pub fn new() -> Self {
        BrahmsHandler {
            shutting_down: false,
            send_queue: SmallVec::new(),
            connection_keep_alive: false,
            ongoing_pull_request: None,
            pull_response_flushes: SmallVec::new(),
        }
    }
}

impl<TSubstream> ProtocolsHandler for BrahmsHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = BrahmsHandlerIn;
    type OutEvent = BrahmsHandlerEvent;
    type Substream = TSubstream;
    type Error = io::Error;
    type InboundProtocol = BrahmsListen;
    type OutboundProtocol = EitherUpgrade<BrahmsPushRequest, BrahmsPullRequestRequest>;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        BrahmsListen::default()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        request: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output,
    ) {
        match request {
            BrahmsListenOut::Push(addresses) => {
                self.send_queue
                    .push(ProtocolsHandlerEvent::Custom(BrahmsHandlerEvent::Push {
                        addresses,
                    }));
            }
            BrahmsListenOut::PullRequest(request) => {
                self.ongoing_pull_request = Some(request);
                self.send_queue.push(ProtocolsHandlerEvent::Custom(
                    BrahmsHandlerEvent::PullRequest,
                ));
            }
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        response: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        let list = match response {
            EitherOutput::First(()) => return,
            EitherOutput::Second(list) => list,
        };

        self.send_queue.push(ProtocolsHandlerEvent::Custom(
            BrahmsHandlerEvent::PullResult { list },
        ));
    }

    #[inline]
    fn inject_event(&mut self, message: BrahmsHandlerIn) {
        match message {
            BrahmsHandlerIn::Event(BrahmsHandlerEvent::Push { addresses }) => {
                self.send_queue
                    .push(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        upgrade: EitherUpgrade::A(BrahmsPushRequest { addresses }),
                        info: (),
                    });
            }
            BrahmsHandlerIn::Event(BrahmsHandlerEvent::PullRequest) => {
                self.send_queue
                    .push(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        upgrade: EitherUpgrade::B(BrahmsPullRequestRequest {}),
                        info: (),
                    });
            }
            BrahmsHandlerIn::Event(BrahmsHandlerEvent::PullResult { list }) => {
                if let Some(request) = self.ongoing_pull_request.take() {
                    let flush = request.respond(list);
                    self.pull_response_flushes.push(flush);
                }
            }
            BrahmsHandlerIn::EnableKeepAlive => {
                self.connection_keep_alive = true;
            }
            BrahmsHandlerIn::DisableKeepAlive => {
                self.connection_keep_alive = false;
            }
        }
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<EitherError<io::Error, Box<error::Error + Send + Sync>>>) {
    }

    #[inline]
    fn connection_keep_alive(&self) -> bool {
        self.connection_keep_alive
    }

    #[inline]
    fn shutdown(&mut self) {
        self.shutting_down = true;
    }

    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        Self::Error,
    > {
        if self.shutting_down {
            return Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown));
        }

        if !self.send_queue.is_empty() {
            let event = self.send_queue.remove(0);
            return Ok(Async::Ready(event));
        }

        // We remove each element from `pull_response_flushes` one by one and add them back if
        // not ready.
        for n in (0..self.pull_response_flushes.len()).rev() {
            let mut fut = self.pull_response_flushes.swap_remove(n);
            match fut.poll() {
                Ok(Async::NotReady) => {
                    self.pull_response_flushes.push(fut);
                }
                Ok(Async::Ready(())) | Err(_) => {}
            }
        }

        Ok(Async::NotReady)
    }
}

impl<TSubstream> fmt::Debug for BrahmsHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("BrahmsHandler")
            .field("shutting_down", &self.shutting_down)
            .field("send_queue", &self.send_queue.len())
            .field("ongoing_pull_request", &self.ongoing_pull_request.is_some())
            .field("pull_response_flushes", &self.pull_response_flushes.len())
            .finish()
    }
}
