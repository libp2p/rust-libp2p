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

use crate::protocols_handler::{
    KeepAlive,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr,
    SubstreamProtocol
};
use futures::prelude::*;
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade};
use smallvec::SmallVec;
use std::{error, marker::PhantomData, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::Instant;

/// Implementation of `ProtocolsHandler` that opens a new substream for each individual message.
///
/// This struct is meant to be a helper for other implementations to use.
// TODO: Debug
pub struct OneShotHandler<TSubstream, TInProto, TOutProto, TOutEvent>
where
    TOutProto: OutboundUpgrade<TSubstream>,
{
    /// The upgrade for inbound substreams.
    listen_protocol: SubstreamProtocol<TInProto>,
    /// If `Some`, something bad happened and we should shut down the handler with an error.
    pending_error:
        Option<ProtocolsHandlerUpgrErr<<TOutProto as OutboundUpgrade<TSubstream>>::Error>>,
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
    /// After the given duration has elapsed, an inactive connection will shutdown.
    inactive_timeout: Duration,
    /// Pin the `TSubstream` generic.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream, TInProto, TOutProto, TOutEvent>
    OneShotHandler<TSubstream, TInProto, TOutProto, TOutEvent>
where
    TOutProto: OutboundUpgrade<TSubstream>,
{
    /// Creates a `OneShotHandler`.
    #[inline]
    pub fn new(
        listen_protocol: SubstreamProtocol<TInProto>,
        inactive_timeout: Duration
    ) -> Self {
        OneShotHandler {
            listen_protocol,
            pending_error: None,
            events_out: SmallVec::new(),
            dial_queue: SmallVec::new(),
            dial_negotiated: 0,
            max_dial_negotiated: 8,
            keep_alive: KeepAlive::Yes,
            inactive_timeout,
            marker: PhantomData,
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

impl<TSubstream, TInProto, TOutProto, TOutEvent> Default
    for OneShotHandler<TSubstream, TInProto, TOutProto, TOutEvent>
where
    TOutProto: OutboundUpgrade<TSubstream>,
    TInProto: InboundUpgrade<TSubstream> + Default,
{
    #[inline]
    fn default() -> Self {
        OneShotHandler::new(SubstreamProtocol::new(Default::default()), Duration::from_secs(10))
    }
}

impl<TSubstream, TInProto, TOutProto, TOutEvent> ProtocolsHandler
    for OneShotHandler<TSubstream, TInProto, TOutProto, TOutEvent>
where
    TSubstream: AsyncRead + AsyncWrite,
    TInProto: InboundUpgrade<TSubstream>,
    TOutProto: OutboundUpgrade<TSubstream>,
    TInProto::Output: Into<TOutEvent>,
    TOutProto::Output: Into<TOutEvent>,
    TOutProto::Error: error::Error + 'static,
    SubstreamProtocol<TInProto>: Clone,
{
    type InEvent = TOutProto;
    type OutEvent = TOutEvent;
    type Error = ProtocolsHandlerUpgrErr<
        <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error,
    >;
    type Substream = TSubstream;
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
        out: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output,
    ) {
        // If we're shutting down the connection for inactivity, reset the timeout.
        if !self.keep_alive.is_yes() {
            self.keep_alive = KeepAlive::Until(Instant::now() + self.inactive_timeout);
        }

        self.events_out.push(out.into());
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        out: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        self.dial_negotiated -= 1;

        if self.dial_negotiated == 0 && self.dial_queue.is_empty() {
            self.keep_alive = KeepAlive::Until(Instant::now() + self.inactive_timeout);
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
            <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Error,
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
    ) -> Poll<
        ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>,
        Self::Error,
    > {
        if let Some(err) = self.pending_error.take() {
            return Err(err);
        }

        if !self.events_out.is_empty() {
            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(
                self.events_out.remove(0),
            )));
        } else {
            self.events_out.shrink_to_fit();
        }

        if !self.dial_queue.is_empty() {
            if self.dial_negotiated < self.max_dial_negotiated {
                self.dial_negotiated += 1;
                return Ok(Async::Ready(
                    ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(self.dial_queue.remove(0)),
                        info: (),
                    },
                ));
            }
        } else {
            self.dial_queue.shrink_to_fit();
        }

        Ok(Async::NotReady)
    }
}
