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
use core::PeerId;
use core::nodes::handled_node::NodeHandlerEndpoint;
use core::nodes::protocols_handler::{ProtocolsHandler, ProtocolsHandlerEvent};
use core::ConnectionUpgrade;
use protocol::{RelayOutput, RelayConfig};
use std::io;
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;

/// Protocol handler that identifies the remote at a regular period.
pub struct RelayHandler<TSubstream> {
    /// Configuration for the protocol.
    config: RelayConfig,

    /// True if wanting to shut down.
    shutdown: bool,

    /// List of futures that process relaying data from the remote node to a destination.
    active_relays: Vec<Box<Future<Item = (), Error = io::Error> + Send>>,

    /// Queue of events to return when polled.
    queued_events: Vec<RelayHandlerEvent<TSubstream>>,
}

/// Event produced by the periodic identifier.
#[derive(Debug)]
pub enum RelayHandlerEvent<TSubstream> {
    /// The remote is a relay and is relaying a connection to us. In other words, we are used as
    /// destination.
    Destination {
        /// I/O to the source.
        stream: TSubstream,
        /// Peer id of the source.
        src_peer_id: PeerId,
    },
}

impl<TSubstream> RelayHandler<TSubstream> {
    /// Builds a new `RelayHandler`.
    #[inline]
    pub fn new() -> Self {
        RelayHandler {
            config: RelayConfig::new(),
            shutdown: false,
            active_relays: Vec::new(),
            queued_events: Vec::new(),
        }
    }
}

impl<TSubstream> ProtocolsHandler for RelayHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static, // TODO: remove useless bounds
{
    type InEvent = Void;
    type OutEvent = RelayHandlerEvent<TSubstream>;
    type Substream = TSubstream;
    type Protocol = RelayConfig;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::Protocol {
        self.config.clone()
    }

    fn inject_fully_negotiated(
        &mut self,
        protocol: <Self::Protocol as ConnectionUpgrade<TSubstream>>::Output,
        _endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        debug_assert!(_endpoint.is_listener());

        match protocol {
            RelayOutput::HopRequest(request) => unimplemented!(),
            RelayOutput::Stream { stream, src_peer_id } => {
                let ev = RelayHandlerEvent::Destination { stream, src_peer_id };
                self.queued_events.push(ev);
            },
        }
    }

    #[inline]
    fn inject_event(&mut self, _: &Self::InEvent) {}

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: io::Error) {
        // TODO: report?
    }

    #[inline]
    fn shutdown(&mut self) {
        self.shutdown = true;
    }

    fn poll(
        &mut self,
    ) -> Poll<
        Option<
            ProtocolsHandlerEvent<
                Self::Protocol,
                Self::OutboundOpenInfo,
                RelayHandlerEvent<TSubstream>,
            >,
        >,
        io::Error,
    > {
        // Report the queued events.
        if !self.queued_events.is_empty() {
            let event = self.queued_events.remove(0);
            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(event))));
        }

        // We remove each element from `active_relays` one by one and add them back.
        for n in (0..self.active_relays.len()).rev() {
            let mut relay = self.active_relays.swap_remove(n);
            match relay.poll() {
                // Don't add back the relay if it's finished or errors.
                Ok(Async::Ready(())) | Err(_) => {},
                Ok(Async::NotReady) => self.active_relays.push(relay),
            }
        }

        // Shut down process.
        if self.shutdown {
            self.active_relays.clear();     // TODO: not a proper shutdown
            return Ok(Async::Ready(None));
        }

        Ok(Async::NotReady)
    }
}
