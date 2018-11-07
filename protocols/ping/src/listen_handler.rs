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

use arrayvec::ArrayVec;
use futures::prelude::*;
use libp2p_core::{
    nodes::{NodeHandlerEndpoint, ProtocolsHandler, ProtocolsHandlerEvent},
    ConnectionUpgrade,
};
use protocol::{Ping, PingListener, PingOutput};
use std::io;
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;

/// Handler for handling pings received from a remote.
pub struct PingListenHandler<TSubstream> {
    /// Configuration for the ping protocol.
    ping_config: Ping<()>,

    /// The ping substreams that were opened by the remote.
    /// Note that we only accept a certain number of substreams, after which we refuse new ones
    /// to avoid being DDoSed.
    ping_in_substreams: ArrayVec<[PingListener<TSubstream>; 8]>,

    /// If true, we're in the shutdown process and we shouldn't accept new substreams.
    shutdown: bool,
}

impl<TSubstream> PingListenHandler<TSubstream> {
    /// Builds a new `PingListenHandler`.
    pub fn new() -> PingListenHandler<TSubstream> {
        PingListenHandler {
            ping_config: Default::default(),
            shutdown: false,
            ping_in_substreams: ArrayVec::new(),
        }
    }
}

impl<TSubstream> Default for PingListenHandler<TSubstream> {
    #[inline]
    fn default() -> Self {
        PingListenHandler::new()
    }
}

impl<TSubstream> ProtocolsHandler for PingListenHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = Void;
    type OutEvent = Void;
    type Substream = TSubstream;
    type Protocol = Ping<()>;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::Protocol {
        self.ping_config
    }

    fn inject_fully_negotiated(
        &mut self,
        protocol: <Self::Protocol as ConnectionUpgrade<TSubstream>>::Output,
        _endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        if self.shutdown {
            return;
        }

        match protocol {
            PingOutput::Pinger(_) => {
                debug_assert!(false, "Received an unexpected outgoing ping substream");
            }
            PingOutput::Ponger(listener) => {
                debug_assert!(_endpoint.is_listener());
                // Try insert the element, but don't care if the list is full.
                let _ = self.ping_in_substreams.try_push(listener);
            }
        }
    }

    #[inline]
    fn inject_event(&mut self, _: Self::InEvent) {}

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: io::Error) {}

    #[inline]
    fn shutdown(&mut self) {
        for ping in self.ping_in_substreams.iter_mut() {
            ping.shutdown();
        }

        self.shutdown = true;
    }

    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::Protocol, Self::OutboundOpenInfo, Self::OutEvent>>,
        io::Error,
    > {
        // Removes each substream one by one, and pushes them back if they're not ready (which
        // should be the case 99% of the time).
        for n in (0..self.ping_in_substreams.len()).rev() {
            let mut ping = self.ping_in_substreams.swap_remove(n);
            match ping.poll() {
                Ok(Async::Ready(())) => {}
                Ok(Async::NotReady) => self.ping_in_substreams.push(ping),
                Err(err) => warn!(target: "sub-libp2p", "Remote ping substream errored: {:?}", err),
            }
        }

        // Special case if shutting down.
        if self.shutdown && self.ping_in_substreams.is_empty() {
            return Ok(Async::Ready(None));
        }

        Ok(Async::NotReady)
    }
}
