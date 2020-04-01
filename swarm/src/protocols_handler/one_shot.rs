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

/// Implementation of `ProtocolsHandler` that opens a new substream for each individual message.
///
/// This struct is meant to be a helper for other implementations to use.
// TODO: Debug
pub struct OneShotHandler<TInProto, TOutProto, TOutEvent>
where
    TOutProto: OutboundUpgradeSend,
{
    /// The upgrade for inbound substreams.
    listen_protocol: SubstreamProtocol<TInProto>,
    /// If `Some`, something bad happened and we should shut down the handler with an error.
    pending_error:
        Option<ProtocolsHandlerUpgrErr<<TOutProto as OutboundUpgradeSend>::Error>>,
    /// Queue of events to produce in `poll()`.
    events_out: SmallVec<[TOutEvent; 4]>,
    /// Queue of outbound substreams to open.
    dial_queue: SmallVec<[TOutProto; 4]>,
    /// Current number of concurrent outbound substreams being opened.
    dial_negotiated: u32,
    /// Maximum number of concurrent outbound substreams being opened. Value is never modified.
    max_dial_negotiated: u32,
    /// Value to return from `connection_keep_alive`.
    keep_alive: KeepAlive,
    /// The configuration container for the handler
    config: OneShotHandlerConfig,
}

impl<TInProto, TOutProto, TOutEvent>
    OneShotHandler<TInProto, TOutProto, TOutEvent>
where
    TOutProto: OutboundUpgradeSend,
{
    /// Creates a `OneShotHandler`.
    #[inline]
    pub fn new(
        listen_protocol: SubstreamProtocol<TInProto>,
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
            config
        }
    }

    /// Returns the number of pending requests.
    #[inline]
    pub fn pending_requests(&self) -> u32 {
        self.dial_negotiated + self.dial_queue.len() as u32
    }

    /// Returns a reference to the listen protocol configuration.
    ///
    /// > **Note**: If you modify the protocol, modifications will only applies to future inbound
    /// >           substreams, not the ones already being negotiated.
    #[inline]
    pub fn listen_protocol_ref(&self) -> &SubstreamProtocol<TInProto> {
        &self.listen_protocol
    }

    /// Returns a mutable reference to the listen protocol configuration.
    ///
    /// > **Note**: If you modify the protocol, modifications will only applies to future inbound
    /// >           substreams, not the ones already being negotiated.
    #[inline]
    pub fn listen_protocol_mut(&mut self) -> &mut SubstreamProtocol<TInProto> {
        &mut self.listen_protocol
    }

    /// Opens an outbound substream with `upgrade`.
    #[inline]
    pub fn send_request(&mut self, upgrade: TOutProto) {
        self.keep_alive = KeepAlive::Yes;
        self.dial_queue.push(upgrade);
    }
}

impl<TInProto, TOutProto, TOutEvent> Default
    for OneShotHandler<TInProto, TOutProto, TOutEvent>
where
    TOutProto: OutboundUpgradeSend,
    TInProto: InboundUpgradeSend + Default,
{
    #[inline]
    fn default() -> Self {
        OneShotHandler::new(
            SubstreamProtocol::new(Default::default()),
            OneShotHandlerConfig::default()
        )
    }
}

impl<TInProto, TOutProto, TOutEvent> ProtocolsHandler
    for OneShotHandler<TInProto, TOutProto, TOutEvent>
where
    TInProto: InboundUpgradeSend + Send + 'static,
    TOutProto: OutboundUpgradeSend,
    TInProto::Output: Into<TOutEvent>,
    TOutProto::Output: Into<TOutEvent>,
    TOutProto::Error: error::Error + Send + 'static,
    SubstreamProtocol<TInProto>: Clone,
    TOutEvent: Send + 'static,
{
    type InEvent = TOutProto;
    type OutEvent = TOutEvent;
    type Error = ProtocolsHandlerUpgrErr<
        <Self::OutboundProtocol as OutboundUpgradeSend>::Error,
    >;
    type InboundProtocol = TInProto;
    type OutboundProtocol = TOutProto;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        self.listen_protocol.clone()
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        out: <Self::InboundProtocol as InboundUpgradeSend>::Output,
    ) {
        // If we're shutting down the connection for inactivity, reset the timeout.
        if !self.keep_alive.is_yes() {
            self.keep_alive = KeepAlive::Until(Instant::now() + self.config.inactive_timeout);
        }

        self.events_out.push(out.into());
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        out: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        self.dial_negotiated -= 1;

        if self.dial_negotiated == 0 && self.dial_queue.is_empty() {
            self.keep_alive = KeepAlive::Until(Instant::now() + self.config.inactive_timeout);
        }

        self.events_out.push(out.into());
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        self.send_request(event);
    }

    #[inline]
    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgradeSend>::Error,
        >,
    ) {
        if self.pending_error.is_none() {
            self.pending_error = Some(error);
        }
    }

    #[inline]
    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        _: &mut Context,
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>,
    > {
        if let Some(err) = self.pending_error.take() {
            return Poll::Ready(ProtocolsHandlerEvent::Close(err));
        }

        if !self.events_out.is_empty() {
            return Poll::Ready(ProtocolsHandlerEvent::Custom(
                self.events_out.remove(0),
            ));
        } else {
            self.events_out.shrink_to_fit();
        }

        if !self.dial_queue.is_empty() {
            if self.dial_negotiated < self.max_dial_negotiated {
                self.dial_negotiated += 1;
                return Poll::Ready(
                    ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(self.dial_queue.remove(0))
                            .with_timeout(self.config.substream_timeout),
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
    /// After the given duration has elapsed, an inactive connection will shutdown.
    inactive_timeout: Duration,
    /// Timeout duration for each newly opened outbound substream.
    substream_timeout: Duration,
}

impl Default for OneShotHandlerConfig {
    fn default() -> Self {
        let inactive_timeout = Duration::from_secs(10);
        let substream_timeout = Duration::from_secs(10);
        OneShotHandlerConfig { inactive_timeout, substream_timeout }
    }
}

