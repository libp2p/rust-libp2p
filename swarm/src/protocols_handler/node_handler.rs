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

use crate::protocols_handler::{
    KeepAlive,
    ProtocolsHandler,
    IntoProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr
};
use futures::prelude::*;
use libp2p_core::{
    ConnectedPoint,
    PeerId,
    nodes::collection::ConnectionInfo,
    nodes::handled_node::{IntoNodeHandler, NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent},
    upgrade::{self, InboundUpgradeApply, OutboundUpgradeApply}
};
use std::{error, fmt, time::Duration};
use wasm_timer::{Delay, Timeout};

/// Prototype for a `NodeHandlerWrapper`.
pub struct NodeHandlerWrapperBuilder<TIntoProtoHandler> {
    /// The underlying handler.
    handler: TIntoProtoHandler,
}

impl<TIntoProtoHandler> NodeHandlerWrapperBuilder<TIntoProtoHandler>
where
    TIntoProtoHandler: IntoProtocolsHandler
{
    /// Builds a `NodeHandlerWrapperBuilder`.
    #[inline]
    pub(crate) fn new(handler: TIntoProtoHandler) -> Self {
        NodeHandlerWrapperBuilder {
            handler,
        }
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
            queued_dial_upgrades: Vec::new(),
            unique_dial_upgrade_id: 0,
            shutdown: Shutdown::None,
        }
    }
}

impl<TIntoProtoHandler, TProtoHandler, TConnInfo> IntoNodeHandler<(TConnInfo, ConnectedPoint)>
    for NodeHandlerWrapperBuilder<TIntoProtoHandler>
where
    TIntoProtoHandler: IntoProtocolsHandler<Handler = TProtoHandler>,
    TProtoHandler: ProtocolsHandler,
    TConnInfo: ConnectionInfo<PeerId = PeerId>,
{
    type Handler = NodeHandlerWrapper<TIntoProtoHandler::Handler>;

    fn into_handler(self, remote_info: &(TConnInfo, ConnectedPoint)) -> Self::Handler {
        NodeHandlerWrapper {
            handler: self.handler.into_handler(&remote_info.0.peer_id(), &remote_info.1),
            negotiating_in: Vec::new(),
            negotiating_out: Vec::new(),
            queued_dial_upgrades: Vec::new(),
            unique_dial_upgrade_id: 0,
            shutdown: Shutdown::None,
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
    /// For each outbound substream request, how to upgrade it. The first element of the tuple
    /// is the unique identifier (see `unique_dial_upgrade_id`).
    queued_dial_upgrades: Vec<(u64, (upgrade::Version, TProtoHandler::OutboundProtocol))>,
    /// Unique identifier assigned to each queued dial upgrade.
    unique_dial_upgrade_id: u64,
    /// The currently planned connection & handler shutdown.
    shutdown: Shutdown,
}

/// The options for a planned connection & handler shutdown.
///
/// A shutdown is planned anew based on the the return value of
/// [`ProtocolsHandler::connection_keep_alive`] of the underlying handler
/// after every invocation of [`ProtocolsHandler::poll`].
///
/// A planned shutdown is always postponed for as long as there are ingoing
/// or outgoing substreams being negotiated, i.e. it is a graceful, "idle"
/// shutdown.
enum Shutdown {
    /// No shutdown is planned.
    None,
    /// A shut down is planned as soon as possible.
    Asap,
    /// A shut down is planned for when a `Delay` has elapsed.
    Later(Delay)
}

/// Error generated by the `NodeHandlerWrapper`.
#[derive(Debug)]
pub enum NodeHandlerWrapperError<TErr> {
    /// Error generated by the handler.
    Handler(TErr),
    /// The connection has been deemed useless and has been closed.
    UselessTimeout,
}

impl<TErr> From<TErr> for NodeHandlerWrapperError<TErr> {
    fn from(err: TErr) -> NodeHandlerWrapperError<TErr> {
        NodeHandlerWrapperError::Handler(err)
    }
}

impl<TErr> fmt::Display for NodeHandlerWrapperError<TErr>
where
    TErr: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeHandlerWrapperError::Handler(err) => write!(f, "{}", err),
            NodeHandlerWrapperError::UselessTimeout =>
                write!(f, "Node has been closed due to inactivity"),
        }
    }
}

impl<TErr> error::Error for NodeHandlerWrapperError<TErr>
where
    TErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            NodeHandlerWrapperError::Handler(err) => Some(err),
            NodeHandlerWrapperError::UselessTimeout => None,
        }
    }
}

impl<TProtoHandler> NodeHandler for NodeHandlerWrapper<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler,
{
    type InEvent = TProtoHandler::InEvent;
    type OutEvent = TProtoHandler::OutEvent;
    type Error = NodeHandlerWrapperError<TProtoHandler::Error>;
    type Substream = TProtoHandler::Substream;
    // The first element of the tuple is the unique upgrade identifier
    // (see `unique_dial_upgrade_id`).
    type OutboundOpenInfo = (u64, TProtoHandler::OutboundOpenInfo, Duration);

    fn inject_substream(
        &mut self,
        substream: Self::Substream,
        endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        match endpoint {
            NodeHandlerEndpoint::Listener => {
                let protocol = self.handler.listen_protocol();
                let timeout = protocol.timeout().clone();
                let upgrade = upgrade::apply_inbound(substream, protocol.into_upgrade().1);
                let with_timeout = Timeout::new(upgrade, timeout);
                self.negotiating_in.push(with_timeout);
            }
            NodeHandlerEndpoint::Dialer((upgrade_id, user_data, timeout)) => {
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

                let (_, (version, upgrade)) = self.queued_dial_upgrades.remove(pos);
                let upgrade = upgrade::apply_outbound(substream, upgrade, version);
                let with_timeout = Timeout::new(upgrade, timeout);
                self.negotiating_out.push((user_data, with_timeout));
            }
        }
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        self.handler.inject_event(event);
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
                            timer, inner}; is_elapsed and is_timer are both false; error is \
                            inner; QED");
                        ProtocolsHandlerUpgrErr::Upgrade(err)
                    };

                    self.handler.inject_dial_upgrade_error(upgr_info, err);
                }
            }
        }

        // Poll the handler at the end so that we see the consequences of the method
        // calls on `self.handler`.
        let poll_result = self.handler.poll()?;

        // Ask the handler whether it wants the connection (and the handler itself)
        // to be kept alive, which determines the planned shutdown, if any.
        match (&mut self.shutdown, self.handler.connection_keep_alive()) {
            (Shutdown::Later(d), KeepAlive::Until(t)) =>
                if d.deadline() != t {
                    d.reset(t)
                },
            (_, KeepAlive::Until(t)) => self.shutdown = Shutdown::Later(Delay::new(t)),
            (_, KeepAlive::No) => self.shutdown = Shutdown::Asap,
            (_, KeepAlive::Yes) => self.shutdown = Shutdown::None
        };

        match poll_result {
            Async::Ready(ProtocolsHandlerEvent::Custom(event)) => {
                return Ok(Async::Ready(NodeHandlerEvent::Custom(event)));
            }
            Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol,
                info,
            }) => {
                let id = self.unique_dial_upgrade_id;
                let timeout = protocol.timeout().clone();
                self.unique_dial_upgrade_id += 1;
                self.queued_dial_upgrades.push((id, protocol.into_upgrade()));
                return Ok(Async::Ready(
                    NodeHandlerEvent::OutboundSubstreamRequest((id, info, timeout)),
                ));
            }
            Async::NotReady => (),
        };

        // Check if the connection (and handler) should be shut down.
        // As long as we're still negotiating substreams, shutdown is always postponed.
        if self.negotiating_in.is_empty() && self.negotiating_out.is_empty() {
            match self.shutdown {
                Shutdown::None => {},
                Shutdown::Asap => return Err(NodeHandlerWrapperError::UselessTimeout),
                Shutdown::Later(ref mut delay) => match delay.poll() {
                    Ok(Async::Ready(_)) | Err(_) =>
                        return Err(NodeHandlerWrapperError::UselessTimeout),
                    Ok(Async::NotReady) => {}
                }
            }
        }

        Ok(Async::NotReady)
    }
}
