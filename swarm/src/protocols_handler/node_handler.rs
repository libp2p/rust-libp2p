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

use crate::upgrade::SendWrapper;
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
    muxing::StreamMuxerBox,
    nodes::Substream,
    nodes::collection::ConnectionInfo,
    nodes::handled_node::{IntoNodeHandler, NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent},
    upgrade::{self, InboundUpgradeApply, OutboundUpgradeApply}
};
use std::{error, fmt, pin::Pin, task::Context, task::Poll, time::Duration};
use wasm_timer::{Delay, Instant};

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
        Vec<(InboundUpgradeApply<Substream<StreamMuxerBox>, SendWrapper<TProtoHandler::InboundProtocol>>, Delay)>,
    /// Futures that upgrade outgoing substreams. The first element of the tuple is the userdata
    /// to pass back once successfully opened.
    negotiating_out: Vec<(
        TProtoHandler::OutboundOpenInfo,
        OutboundUpgradeApply<Substream<StreamMuxerBox>, SendWrapper<TProtoHandler::OutboundProtocol>>,
        Delay,
    )>,
    /// For each outbound substream request, how to upgrade it. The first element of the tuple
    /// is the unique identifier (see `unique_dial_upgrade_id`).
    queued_dial_upgrades: Vec<(u64, (upgrade::Version, SendWrapper<TProtoHandler::OutboundProtocol>))>,
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
    Later(Delay, Instant)
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
    type Substream = Substream<StreamMuxerBox>;
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
                let upgrade = upgrade::apply_inbound(substream, SendWrapper(protocol.into_upgrade().1));
                let timeout = Delay::new(timeout);
                self.negotiating_in.push((upgrade, timeout));
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
                let timeout = Delay::new(timeout);
                self.negotiating_out.push((user_data, upgrade, timeout));
            }
        }
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        self.handler.inject_event(event);
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<Result<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>, Self::Error>> {
        // Continue negotiation of newly-opened substreams on the listening side.
        // We remove each element from `negotiating_in` one by one and add them back if not ready.
        for n in (0..self.negotiating_in.len()).rev() {
            let (mut in_progress, mut timeout) = self.negotiating_in.swap_remove(n);
            match Future::poll(Pin::new(&mut timeout), cx) {
                Poll::Ready(_) => continue,
                Poll::Pending => {},
            }
            match Future::poll(Pin::new(&mut in_progress), cx) {
                Poll::Ready(Ok(upgrade)) =>
                    self.handler.inject_fully_negotiated_inbound(upgrade),
                Poll::Pending => self.negotiating_in.push((in_progress, timeout)),
                // TODO: return a diagnostic event?
                Poll::Ready(Err(_err)) => {}
            }
        }

        // Continue negotiation of newly-opened substreams.
        // We remove each element from `negotiating_out` one by one and add them back if not ready.
        for n in (0..self.negotiating_out.len()).rev() {
            let (upgr_info, mut in_progress, mut timeout) = self.negotiating_out.swap_remove(n);
            match Future::poll(Pin::new(&mut timeout), cx) {
                Poll::Ready(Ok(_)) => {
                    let err = ProtocolsHandlerUpgrErr::Timeout;
                    self.handler.inject_dial_upgrade_error(upgr_info, err);
                    continue;
                },
                Poll::Ready(Err(_)) => {
                    let err = ProtocolsHandlerUpgrErr::Timer;
                    self.handler.inject_dial_upgrade_error(upgr_info, err);
                    continue;
                },
                Poll::Pending => {},
            }
            match Future::poll(Pin::new(&mut in_progress), cx) {
                Poll::Ready(Ok(upgrade)) => {
                    self.handler.inject_fully_negotiated_outbound(upgrade, upgr_info);
                }
                Poll::Pending => {
                    self.negotiating_out.push((upgr_info, in_progress, timeout));
                }
                Poll::Ready(Err(err)) => {
                    let err = ProtocolsHandlerUpgrErr::Upgrade(err);
                    self.handler.inject_dial_upgrade_error(upgr_info, err);
                }
            }
        }

        // Poll the handler at the end so that we see the consequences of the method
        // calls on `self.handler`.
        let poll_result = self.handler.poll(cx);

        // Ask the handler whether it wants the connection (and the handler itself)
        // to be kept alive, which determines the planned shutdown, if any.
        match (&mut self.shutdown, self.handler.connection_keep_alive()) {
            (Shutdown::Later(timer, deadline), KeepAlive::Until(t)) =>
                if *deadline != t {
                    *deadline = t;
                    timer.reset_at(t)
                },
            (_, KeepAlive::Until(t)) => self.shutdown = Shutdown::Later(Delay::new_at(t), t),
            (_, KeepAlive::No) => self.shutdown = Shutdown::Asap,
            (_, KeepAlive::Yes) => self.shutdown = Shutdown::None
        };

        match poll_result {
            Poll::Ready(ProtocolsHandlerEvent::Custom(event)) => {
                return Poll::Ready(Ok(NodeHandlerEvent::Custom(event)));
            }
            Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol,
                info,
            }) => {
                let id = self.unique_dial_upgrade_id;
                let timeout = protocol.timeout().clone();
                self.unique_dial_upgrade_id += 1;
                let (version, upgrade) = protocol.into_upgrade();
                self.queued_dial_upgrades.push((id, (version, SendWrapper(upgrade))));
                return Poll::Ready(Ok(
                    NodeHandlerEvent::OutboundSubstreamRequest((id, info, timeout)),
                ));
            }
            Poll::Ready(ProtocolsHandlerEvent::Close(err)) => return Poll::Ready(Err(err.into())),
            Poll::Pending => (),
        };

        // Check if the connection (and handler) should be shut down.
        // As long as we're still negotiating substreams, shutdown is always postponed.
        if self.negotiating_in.is_empty() && self.negotiating_out.is_empty() {
            match self.shutdown {
                Shutdown::None => {},
                Shutdown::Asap => return Poll::Ready(Err(NodeHandlerWrapperError::UselessTimeout)),
                Shutdown::Later(ref mut delay, _) => match Future::poll(Pin::new(delay), cx) {
                    Poll::Ready(_) => return Poll::Ready(Err(NodeHandlerWrapperError::UselessTimeout)),
                    Poll::Pending => {}
                }
            }
        }

        Poll::Pending
    }
}
