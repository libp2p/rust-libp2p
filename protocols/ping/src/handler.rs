// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::protocol;
use futures::prelude::*;
use libp2p_core::ProtocolsHandlerEvent;
use libp2p_core::protocols_handler::{KeepAlive, OneShotHandler, ProtocolsHandler, ProtocolsHandlerUpgrErr};
use std::{io, time::Duration, time::Instant};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Delay;

/// Protocol handler that handles pinging the remote at a regular period and answering ping
/// queries.
///
/// If the remote doesn't respond, produces an error that closes the connection.
pub struct PingHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// The actual handler which we delegate the substreams handling to.
    inner: OneShotHandler<TSubstream, protocol::Ping, protocol::Ping, protocol::PingOutput>,
    /// After a ping succeeds, how long before the next ping.
    delay_to_next_ping: Duration,
    /// When the next ping triggers.
    next_ping: Delay,
}

impl<TSubstream> PingHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Builds a new `PingHandler`.
    pub fn new() -> Self {
        // TODO: allow customizing timeout; depends on https://github.com/libp2p/rust-libp2p/issues/864
        PingHandler {
            inner: OneShotHandler::default(),
            next_ping: Delay::new(Instant::now()),
            delay_to_next_ping: Duration::from_secs(15),
        }
    }
}

impl<TSubstream> Default for PingHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    #[inline]
    fn default() -> Self {
        PingHandler::new()
    }
}

impl<TSubstream> ProtocolsHandler for PingHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = void::Void;
    type OutEvent = protocol::PingOutput;
    type Error = ProtocolsHandlerUpgrErr<io::Error>;
    type Substream = TSubstream;
    type InboundProtocol = protocol::Ping;
    type OutboundProtocol = protocol::Ping;
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> Self::InboundProtocol {
        self.inner.listen_protocol()
    }

    fn inject_fully_negotiated_inbound(&mut self, protocol: ()) {
        self.inner.inject_fully_negotiated_inbound(protocol)
    }

    fn inject_fully_negotiated_outbound(&mut self, duration: Duration, info: Self::OutboundOpenInfo) {
        self.inner.inject_fully_negotiated_outbound(duration, info)
    }

    fn inject_event(&mut self, event: void::Void) {
        void::unreachable(event)
    }

    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: ProtocolsHandlerUpgrErr<io::Error>) {
        self.inner.inject_dial_upgrade_error(info, error)
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.inner.connection_keep_alive()
    }

    fn poll(
        &mut self,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        Self::Error,
    > {
        match self.next_ping.poll() {
            Ok(Async::Ready(())) => {
                self.inner.inject_event(protocol::Ping::default());
                self.next_ping.reset(Instant::now() + Duration::from_secs(3600));
            },
            Ok(Async::NotReady) => (),
            Err(_) => (),
        };

        let event = self.inner.poll();
        if let Ok(Async::Ready(ProtocolsHandlerEvent::Custom(protocol::PingOutput::Ping(_)))) = event {
            self.next_ping.reset(Instant::now() + self.delay_to_next_ping);
        }
        event
    }
}
