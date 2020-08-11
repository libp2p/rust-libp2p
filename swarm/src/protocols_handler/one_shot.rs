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

use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend};
use crate::protocols_handler::{
    KeepAlive,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr,
    SubstreamProtocol
};

use smallvec::SmallVec;
use std::{error, task::Context, task::Poll, time::Duration};
use wasm_timer::Instant;

/// A `ProtocolsHandler` that opens a new substream for each request.
// TODO: Debug
pub struct OneShotHandler<TInbound, TOutbound, TEvent>
where
    TOutbound: OutboundUpgradeSend,
{
    /// The upgrade for inbound substreams.
    listen_protocol: SubstreamProtocol<TInbound>,
    /// If `Some`, something bad happened and we should shut down the handler with an error.
    pending_error: Option<ProtocolsHandlerUpgrErr<<TOutbound as OutboundUpgradeSend>::Error>>,
    /// Queue of events to produce in `poll()`.
    events_out: SmallVec<[TEvent; 4]>,
    /// Queue of outbound substreams to open.
    dial_queue: SmallVec<[TOutbound; 4]>,
    /// Current number of concurrent outbound substreams being opened.
    dial_negotiated: u32,
    /// Maximum number of concurrent outbound substreams being opened. Value is never modified.
    max_dial_negotiated: u32,
    /// Value to return from `connection_keep_alive`.
    keep_alive: KeepAlive,
    /// The configuration container for the handler
    config: OneShotHandlerConfig,
}

impl<TInbound, TOutbound, TEvent>
    OneShotHandler<TInbound, TOutbound, TEvent>
where
    TOutbound: OutboundUpgradeSend,
{
    /// Creates a `OneShotHandler`.
    pub fn new(
        listen_protocol: SubstreamProtocol<TInbound>,
        config: OneShotHandlerConfig,
    ) -> Self {
        OneShotHandler {
            listen_protocol,
            pending_error: None,
            events_out: SmallVec::new(),
            dial_queue: SmallVec::new(),
            dial_negotiated: 0,
            max_dial_negotiated: 8,
            keep_alive: KeepAlive::Yes,
            config,
        }
    }

    /// Returns the number of pending requests.
    pub fn pending_requests(&self) -> u32 {
        self.dial_negotiated + self.dial_queue.len() as u32
    }

    /// Returns a reference to the listen protocol configuration.
    ///
    /// > **Note**: If you modify the protocol, modifications will only applies to future inbound
    /// >           substreams, not the ones already being negotiated.
    pub fn listen_protocol_ref(&self) -> &SubstreamProtocol<TInbound> {
        &self.listen_protocol
    }

    /// Returns a mutable reference to the listen protocol configuration.
    ///
    /// > **Note**: If you modify the protocol, modifications will only applies to future inbound
    /// >           substreams, not the ones already being negotiated.
    pub fn listen_protocol_mut(&mut self) -> &mut SubstreamProtocol<TInbound> {
        &mut self.listen_protocol
    }

    /// Opens an outbound substream with `upgrade`.
    pub fn send_request(&mut self, upgrade: TOutbound) {
        self.keep_alive = KeepAlive::Yes;
        self.dial_queue.push(upgrade);
    }
}

impl<TInbound, TOutbound, TEvent> Default
    for OneShotHandler<TInbound, TOutbound, TEvent>
where
    TOutbound: OutboundUpgradeSend,
    TInbound: InboundUpgradeSend + Default,
{
    fn default() -> Self {
        OneShotHandler::new(
            SubstreamProtocol::new(Default::default()),
            OneShotHandlerConfig::default()
        )
    }
}

impl<TInbound, TOutbound, TEvent> ProtocolsHandler
    for OneShotHandler<TInbound, TOutbound, TEvent>
where
    TInbound: InboundUpgradeSend + Send + 'static,
    TOutbound: OutboundUpgradeSend,
    TInbound::Output: Into<TEvent>,
    TOutbound::Output: Into<TEvent>,
    TOutbound::Error: error::Error + Send + 'static,
    SubstreamProtocol<TInbound>: Clone,
    TEvent: Send + 'static,
{
    type InEvent = TOutbound;
    type OutEvent = TEvent;
    type Error = ProtocolsHandlerUpgrErr<
        <Self::OutboundProtocol as OutboundUpgradeSend>::Error,
    >;
    type InboundProtocol = TInbound;
    type OutboundProtocol = TOutbound;
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        self.listen_protocol.clone()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        out: <Self::InboundProtocol as InboundUpgradeSend>::Output,
    ) {
        // If we're shutting down the connection for inactivity, reset the timeout.
        if !self.keep_alive.is_yes() {
            self.keep_alive = KeepAlive::Until(Instant::now() + self.config.keep_alive_timeout);
        }

        self.events_out.push(out.into());
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        out: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        self.dial_negotiated -= 1;

        if self.dial_negotiated == 0 && self.dial_queue.is_empty() {
            self.keep_alive = KeepAlive::Until(Instant::now() + self.config.keep_alive_timeout);
        }

        self.events_out.push(out.into());
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        self.send_request(event);
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _info: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgradeSend>::Error,
        >,
    ) {
        if self.pending_error.is_none() {
            self.pending_error = Some(error);
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>,
    > {
        if let Some(err) = self.pending_error.take() {
            return Poll::Ready(ProtocolsHandlerEvent::Close(err))
        }

        if !self.events_out.is_empty() {
            return Poll::Ready(ProtocolsHandlerEvent::Custom(
                self.events_out.remove(0)
            ));
        } else {
            self.events_out.shrink_to_fit();
        }

        if !self.dial_queue.is_empty() {
            if self.dial_negotiated < self.max_dial_negotiated {
                self.dial_negotiated += 1;
                let upgrade = self.dial_queue.remove(0);
                return Poll::Ready(
                    ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(upgrade)
                            .with_timeout(self.config.outbound_substream_timeout),
                        info: (),
                    },
                );
            }
        } else {
            self.dial_queue.shrink_to_fit();
        }

        Poll::Pending
    }
}

/// Configuration parameters for the `OneShotHandler`
#[derive(Debug)]
pub struct OneShotHandlerConfig {
    /// Keep-alive timeout for idle connections.
    pub keep_alive_timeout: Duration,
    /// Timeout for outbound substream upgrades.
    pub outbound_substream_timeout: Duration,
}

impl Default for OneShotHandlerConfig {
    fn default() -> Self {
        OneShotHandlerConfig {
            keep_alive_timeout: Duration::from_secs(10),
            outbound_substream_timeout: Duration::from_secs(10),
        }
    }
}

