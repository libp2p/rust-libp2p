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

use crate::protocol::{Ping, PingListener};
use arrayvec::ArrayVec;
use futures::prelude::*;
use libp2p_core::{
    InboundUpgrade,
    OutboundUpgrade,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    protocols_handler::ProtocolsHandlerUpgrErr,
    upgrade::DeniedUpgrade
};
use log::warn;
use tokio_io::{AsyncRead, AsyncWrite};
use void::{Void, unreachable};

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
    type Error = Void;
    type Substream = TSubstream;
    type InboundProtocol = Ping<()>;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        self.ping_config
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output
    ) {
        if self.shutdown {
            return;
        }
        let _ = self.ping_in_substreams.try_push(protocol);
    }

    fn inject_fully_negotiated_outbound(&mut self, protocol: Void, _info: Self::OutboundOpenInfo) {
        unreachable(protocol)
    }

    #[inline]
    fn inject_event(&mut self, _: Self::InEvent) {}

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error>) {}

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
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        Self::Error,
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
            return Ok(Async::Ready(ProtocolsHandlerEvent::Shutdown));
        }

        Ok(Async::NotReady)
    }
}
