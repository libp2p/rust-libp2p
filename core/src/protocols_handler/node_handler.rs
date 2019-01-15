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

use crate::{
    PeerId,
    nodes::handled_node::{NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent},
    nodes::handled_node_tasks::IntoNodeHandler,
    protocols_handler::{ProtocolsHandler, IntoProtocolsHandler, ProtocolsHandlerEvent, ProtocolsHandlerUpgrErr},
    upgrade::{
        self,
        OutboundUpgrade,
        InboundUpgradeApply,
        OutboundUpgradeApply,
    }
};
use futures::prelude::*;
use std::time::{Duration, Instant};
use tokio_timer::{Delay, Timeout};

/// Prototype for a `NodeHandlerWrapper`.
pub struct NodeHandlerWrapperBuilder<TIntoProtoHandler> {
    /// The underlying handler.
    handler: TIntoProtoHandler,
    /// Timeout for incoming substreams negotiation.
    in_timeout: Duration,
    /// Timeout for outgoing substreams negotiation.
    out_timeout: Duration,
    /// Time after which a useless connection will be closed.
    useless_timeout: Duration,
}

impl<TIntoProtoHandler> NodeHandlerWrapperBuilder<TIntoProtoHandler>
where
    TIntoProtoHandler: IntoProtocolsHandler
{
    /// Builds a `NodeHandlerWrapperBuilder`.
    #[inline]
    pub(crate) fn new(handler: TIntoProtoHandler, in_timeout: Duration, out_timeout: Duration, useless_timeout: Duration) -> Self {
        NodeHandlerWrapperBuilder {
            handler,
            in_timeout,
            out_timeout,
            useless_timeout,
        }
    }

    /// Sets the timeout to use when negotiating a protocol on an ingoing substream.
    #[inline]
    pub fn with_in_negotiation_timeout(mut self, timeout: Duration) -> Self {
        self.in_timeout = timeout;
        self
    }

    /// Sets the timeout to use when negotiating a protocol on an outgoing substream.
    #[inline]
    pub fn with_out_negotiation_timeout(mut self, timeout: Duration) -> Self {
        self.out_timeout = timeout;
        self
    }

    /// Sets the timeout between the moment `connection_keep_alive()` returns `false` on the
    /// `ProtocolsHandler`, and the moment the connection is closed.
    #[inline]
    pub fn with_useless_timeout(mut self, timeout: Duration) -> Self {
        self.useless_timeout = timeout;
        self
    }

    /// Builds the `NodeHandlerWrapper`.
    #[deprecated(note = "Pass the NodeHandlerWrapperBuilder directly")]
    #[inline]
    pub fn build(self) -> NodeHandlerWrapper<TIntoProtoHandler>
    where TIntoProtoHandler: ProtocolsHandler
    {
        NodeHandlerWrapper {
            handler: self.handler,
            negotiating_in: Vec::new(),
            negotiating_out: Vec::new(),
            in_timeout: self.in_timeout,
            out_timeout: self.out_timeout,
            queued_dial_upgrades: Vec::new(),
            unique_dial_upgrade_id: 0,
            connection_shutdown: None,
            useless_timeout: self.useless_timeout,
        }
    }
}

impl<TIntoProtoHandler, TProtoHandler> IntoNodeHandler for NodeHandlerWrapperBuilder<TIntoProtoHandler>
where
    TIntoProtoHandler: IntoProtocolsHandler<Handler = TProtoHandler>,
    TProtoHandler: ProtocolsHandler,
    // TODO: meh for Debug
    <TProtoHandler::OutboundProtocol as OutboundUpgrade<<TProtoHandler as ProtocolsHandler>::Substream>>::Error: std::fmt::Debug
{
    type Handler = NodeHandlerWrapper<TIntoProtoHandler::Handler>;

    fn into_handler(self, remote_peer_id: &PeerId) -> Self::Handler {
        NodeHandlerWrapper {
            handler: self.handler.into_handler(remote_peer_id),
            negotiating_in: Vec::new(),
            negotiating_out: Vec::new(),
            in_timeout: self.in_timeout,
            out_timeout: self.out_timeout,
            queued_dial_upgrades: Vec::new(),
            unique_dial_upgrade_id: 0,
            connection_shutdown: None,
            useless_timeout: self.useless_timeout,
        }
    }
}

/// Wraps around an implementation of `ProtocolsHandler`, and implements `NodeHandler`.
// TODO: add a caching system for protocols that are supported or not
pub struct NodeHandlerWrapper<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler,
{
    /// The underlying handler.
    handler: TProtoHandler,
    /// Futures that upgrade incoming substreams.
    negotiating_in:
        Vec<Timeout<InboundUpgradeApply<TProtoHandler::Substream, TProtoHandler::InboundProtocol>>>,
    /// Futures that upgrade outgoing substreams. The first element of the tuple is the userdata
    /// to pass back once successfully opened.
    negotiating_out: Vec<(
        TProtoHandler::OutboundOpenInfo,
        Timeout<OutboundUpgradeApply<TProtoHandler::Substream, TProtoHandler::OutboundProtocol>>,
    )>,
    /// Timeout for incoming substreams negotiation.
    in_timeout: Duration,
    /// Timeout for outgoing substreams negotiation.
    out_timeout: Duration,
    /// For each outbound substream request, how to upgrade it. The first element of the tuple
    /// is the unique identifier (see `unique_dial_upgrade_id`).
    queued_dial_upgrades: Vec<(u64, TProtoHandler::OutboundProtocol)>,
    /// Unique identifier assigned to each queued dial upgrade.
    unique_dial_upgrade_id: u64,
    /// When a connection has been deemed useless, will contain `Some` with a `Delay` to when it
    /// should be shut down.
    connection_shutdown: Option<Delay>,
    ///  Timeout after which a useless connection is closed. When the `connection_shutdown` is set
    /// to `Some`, this is the value that is being used.
    useless_timeout: Duration,
}

impl<TProtoHandler> NodeHandler for NodeHandlerWrapper<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler,
    // TODO: meh for Debug
    <TProtoHandler::OutboundProtocol as OutboundUpgrade<<TProtoHandler as ProtocolsHandler>::Substream>>::Error: std::fmt::Debug
{
    type InEvent = TProtoHandler::InEvent;
    type OutEvent = TProtoHandler::OutEvent;
    type Error = TProtoHandler::Error;
    type Substream = TProtoHandler::Substream;
    // The first element of the tuple is the unique upgrade identifier
    // (see `unique_dial_upgrade_id`).
    type OutboundOpenInfo = (u64, TProtoHandler::OutboundOpenInfo);

    fn inject_substream(
        &mut self,
        substream: Self::Substream,
        endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        match endpoint {
            NodeHandlerEndpoint::Listener => {
                let protocol = self.handler.listen_protocol();
                let upgrade = upgrade::apply_inbound(substream, protocol);
                let with_timeout = Timeout::new(upgrade, self.in_timeout);
                self.negotiating_in.push(with_timeout);
            }
            NodeHandlerEndpoint::Dialer((upgrade_id, user_data)) => {
                let pos = match self
                    .queued_dial_upgrades
                    .iter()
                    .position(|(id, _)| id == &upgrade_id)
                {
                    Some(p) => p,
                    None => {
                        debug_assert!(false, "Received an upgrade with an invalid upgrade ID");
                        return;
                    }
                };

                let (_, proto_upgrade) = self.queued_dial_upgrades.remove(pos);
                let upgrade = upgrade::apply_outbound(substream, proto_upgrade);
                let with_timeout = Timeout::new(upgrade, self.out_timeout);
                self.negotiating_out.push((user_data, with_timeout));
            }
        }
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {
        self.handler.inject_inbound_closed();
    }

    fn inject_outbound_closed(&mut self, user_data: Self::OutboundOpenInfo) {
        let pos = match self
            .queued_dial_upgrades
            .iter()
            .position(|(id, _)| id == &user_data.0)
        {
            Some(p) => p,
            None => {
                debug_assert!(
                    false,
                    "Received an outbound closed error with an invalid upgrade ID"
                );
                return;
            }
        };

        self.queued_dial_upgrades.remove(pos);
        self.handler
            .inject_dial_upgrade_error(user_data.1, ProtocolsHandlerUpgrErr::MuxerDeniedSubstream);
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        self.handler.inject_event(event);
    }

    #[inline]
    fn shutdown(&mut self) {
        self.handler.shutdown();
    }

    fn poll(&mut self) -> Poll<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>, Self::Error> {
        // Continue negotiation of newly-opened substreams on the listening side.
        // We remove each element from `negotiating_in` one by one and add them back if not ready.
        for n in (0..self.negotiating_in.len()).rev() {
            let mut in_progress = self.negotiating_in.swap_remove(n);
            match in_progress.poll() {
                Ok(Async::Ready(upgrade)) =>
                    self.handler.inject_fully_negotiated_inbound(upgrade),
                Ok(Async::NotReady) => self.negotiating_in.push(in_progress),
                // TODO: return a diagnostic event?
                Err(_err) => {}
            }
        }

        // Continue negotiation of newly-opened substreams.
        // We remove each element from `negotiating_out` one by one and add them back if not ready.
        for n in (0..self.negotiating_out.len()).rev() {
            let (upgr_info, mut in_progress) = self.negotiating_out.swap_remove(n);
            match in_progress.poll() {
                Ok(Async::Ready(upgrade)) => {
                    self.handler.inject_fully_negotiated_outbound(upgrade, upgr_info);
                }
                Ok(Async::NotReady) => {
                    self.negotiating_out.push((upgr_info, in_progress));
                }
                Err(err) => {
                    let err = if err.is_elapsed() {
                        ProtocolsHandlerUpgrErr::Timeout
                    } else if err.is_timer() {
                        ProtocolsHandlerUpgrErr::Timer
                    } else {
                        debug_assert!(err.is_inner());
                        let err = err.into_inner().expect("Timeout error is one of {elapsed, \
                            timer, inner}; is_inner and is_elapsed are both false; error is \
                            inner; QED");
                        ProtocolsHandlerUpgrErr::Upgrade(err)
                    };

                    self.handler.inject_dial_upgrade_error(upgr_info, err);
                }
            }
        }

        // Poll the handler at the end so that we see the consequences of the method calls on
        // `self.handler`.
        let poll_result = self.handler.poll()?;

        if self.handler.connection_keep_alive() {
            self.connection_shutdown = None;
        } else if self.connection_shutdown.is_none() {
            self.connection_shutdown = Some(Delay::new(Instant::now() + self.useless_timeout));
        }

        match poll_result {
            Async::Ready(ProtocolsHandlerEvent::Custom(event)) => {
                return Ok(Async::Ready(NodeHandlerEvent::Custom(event)));
            }
            Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                upgrade,
                info,
            }) => {
                let id = self.unique_dial_upgrade_id;
                self.unique_dial_upgrade_id += 1;
                self.queued_dial_upgrades.push((id, upgrade));
                return Ok(Async::Ready(
                    NodeHandlerEvent::OutboundSubstreamRequest((id, info)),
                ));
            }
            Async::Ready(ProtocolsHandlerEvent::Shutdown) => {
                return Ok(Async::Ready(NodeHandlerEvent::Shutdown))
            },
            Async::NotReady => (),
        };

        // Check the `connection_shutdown`.
        if let Some(mut connection_shutdown) = self.connection_shutdown.take() {
            // If we're negotiating substreams, let's delay the closing.
            if self.negotiating_in.is_empty() && self.negotiating_out.is_empty() {
                match connection_shutdown.poll() {
                    Ok(Async::Ready(_)) | Err(_) => {
                        return Ok(Async::Ready(NodeHandlerEvent::Shutdown))
                    },
                    Ok(Async::NotReady) => {
                        self.connection_shutdown = Some(connection_shutdown);
                    }
                }
            } else {
                self.connection_shutdown = Some(connection_shutdown);
            }
        }

        Ok(Async::NotReady)
    }
}
