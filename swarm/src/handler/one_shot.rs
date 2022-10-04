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

use crate::handler::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    SubstreamProtocol,
};
use crate::upgrade::{InboundUpgradeSend, OutboundUpgradeSend};
use instant::Instant;
use smallvec::SmallVec;
use std::{error, fmt::Debug, task::Context, task::Poll, time::Duration};

/// A [`ConnectionHandler`] that opens a new substream for each request.
// TODO: Debug
pub struct OneShotHandler<TInbound, TOutbound, TEvent>
where
    TOutbound: OutboundUpgradeSend,
{
    /// The upgrade for inbound substreams.
    listen_protocol: SubstreamProtocol<TInbound, ()>,
    /// If `Some`, something bad happened and we should shut down the handler with an error.
    pending_error: Option<ConnectionHandlerUpgrErr<<TOutbound as OutboundUpgradeSend>::Error>>,
    /// Queue of events to produce in `poll()`.
    events_out: SmallVec<[TEvent; 4]>,
    /// Queue of outbound substreams to open.
    dial_queue: SmallVec<[TOutbound; 4]>,
    /// Current number of concurrent outbound substreams being opened.
    dial_negotiated: u32,
    /// Value to return from `connection_keep_alive`.
    keep_alive: KeepAlive,
    /// The configuration container for the handler
    config: OneShotHandlerConfig,
}

impl<TInbound, TOutbound, TEvent> OneShotHandler<TInbound, TOutbound, TEvent>
where
    TOutbound: OutboundUpgradeSend,
{
    /// Creates a `OneShotHandler`.
    pub fn new(
        listen_protocol: SubstreamProtocol<TInbound, ()>,
        config: OneShotHandlerConfig,
    ) -> Self {
        OneShotHandler {
            listen_protocol,
            pending_error: None,
            events_out: SmallVec::new(),
            dial_queue: SmallVec::new(),
            dial_negotiated: 0,
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
    pub fn listen_protocol_ref(&self) -> &SubstreamProtocol<TInbound, ()> {
        &self.listen_protocol
    }

    /// Returns a mutable reference to the listen protocol configuration.
    ///
    /// > **Note**: If you modify the protocol, modifications will only applies to future inbound
    /// >           substreams, not the ones already being negotiated.
    pub fn listen_protocol_mut(&mut self) -> &mut SubstreamProtocol<TInbound, ()> {
        &mut self.listen_protocol
    }

    /// Opens an outbound substream with `upgrade`.
    pub fn send_request(&mut self, upgrade: TOutbound) {
        self.keep_alive = KeepAlive::Yes;
        self.dial_queue.push(upgrade);
    }
}

impl<TInbound, TOutbound, TEvent> Default for OneShotHandler<TInbound, TOutbound, TEvent>
where
    TOutbound: OutboundUpgradeSend,
    TInbound: InboundUpgradeSend + Default,
{
    fn default() -> Self {
        OneShotHandler::new(
            SubstreamProtocol::new(Default::default(), ()),
            OneShotHandlerConfig::default(),
        )
    }
}

impl<TInbound, TOutbound, TEvent> ConnectionHandler for OneShotHandler<TInbound, TOutbound, TEvent>
where
    TInbound: InboundUpgradeSend + Send + 'static,
    TOutbound: Debug + OutboundUpgradeSend,
    TInbound::Output: Into<TEvent>,
    TOutbound::Output: Into<TEvent>,
    TOutbound::Error: error::Error + Send + 'static,
    SubstreamProtocol<TInbound, ()>: Clone,
    TEvent: Debug + Send + 'static,
{
    type InEvent = TOutbound;
    type OutEvent = TEvent;
    type Error = ConnectionHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>;
    type InboundProtocol = TInbound;
    type OutboundProtocol = TOutbound;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        self.listen_protocol.clone()
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        out: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        (): Self::InboundOpenInfo,
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
        self.events_out.push(out.into());
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        self.send_request(event);
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _info: Self::OutboundOpenInfo,
        error: ConnectionHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>,
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
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(err) = self.pending_error.take() {
            return Poll::Ready(ConnectionHandlerEvent::Close(err));
        }

        if !self.events_out.is_empty() {
            return Poll::Ready(ConnectionHandlerEvent::Custom(self.events_out.remove(0)));
        } else {
            self.events_out.shrink_to_fit();
        }

        if !self.dial_queue.is_empty() {
            if self.dial_negotiated < self.config.max_dial_negotiated {
                self.dial_negotiated += 1;
                let upgrade = self.dial_queue.remove(0);
                return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(upgrade, ())
                        .with_timeout(self.config.outbound_substream_timeout),
                });
            }
        } else {
            self.dial_queue.shrink_to_fit();

            if self.dial_negotiated == 0 && self.keep_alive.is_yes() {
                self.keep_alive = KeepAlive::Until(Instant::now() + self.config.keep_alive_timeout);
            }
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
    /// Maximum number of concurrent outbound substreams being opened.
    pub max_dial_negotiated: u32,
}

impl Default for OneShotHandlerConfig {
    fn default() -> Self {
        OneShotHandlerConfig {
            keep_alive_timeout: Duration::from_secs(10),
            outbound_substream_timeout: Duration::from_secs(10),
            max_dial_negotiated: 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::executor::block_on;
    use futures::future::poll_fn;
    use libp2p_core::upgrade::DeniedUpgrade;
    use void::Void;

    #[test]
    fn do_not_keep_idle_connection_alive() {
        let mut handler: OneShotHandler<_, DeniedUpgrade, Void> = OneShotHandler::new(
            SubstreamProtocol::new(DeniedUpgrade {}, ()),
            Default::default(),
        );

        block_on(poll_fn(|cx| loop {
            if handler.poll(cx).is_pending() {
                return Poll::Ready(());
            }
        }));

        assert!(matches!(
            handler.connection_keep_alive(),
            KeepAlive::Until(_)
        ));
    }
}
