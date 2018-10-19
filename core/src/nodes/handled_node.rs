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

use muxing::StreamMuxer;
use nodes::node::{NodeEvent, NodeStream, Substream};
use futures::{prelude::*, stream::Fuse};
use std::io::Error as IoError;

/// Handler for the substreams of a node.
// TODO: right now it is possible for a node handler to be built, then shut down right after if we
//       realize we dialed the wrong peer for example ; this could be surprising and should either
//       be documented or changed (favouring the "documented" right now)
pub trait NodeHandler {
    /// Custom event that can be received from the outside.
    type InEvent;
    /// Custom event that can be produced by the handler and that will be returned by the swarm.
    type OutEvent;
    /// The type of the substream containing the data.
    type Substream;
    /// Information about a substream. Can be sent to the handler through a `NodeHandlerEndpoint`,
    /// and will be passed back in `inject_substream` or `inject_outbound_closed`.
    type OutboundOpenInfo;

    /// Sends a new substream to the handler.
    ///
    /// The handler is responsible for upgrading the substream to whatever protocol it wants.
    fn inject_substream(&mut self, substream: Self::Substream, endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>);

    /// Indicates to the handler that the inbound part of the muxer has been closed, and that
    /// therefore no more inbound substream will be produced.
    fn inject_inbound_closed(&mut self);

    /// Indicates to the handler that an outbound substream failed to open because the outbound
    /// part of the muxer has been closed.
    fn inject_outbound_closed(&mut self, user_data: Self::OutboundOpenInfo);

    /// Injects an event coming from the outside into the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Indicates that the node that it should shut down. After that, it is expected that `poll()`
    /// returns `Ready(None)` as soon as possible.
    ///
    /// This method allows an implementation to perform a graceful shutdown of the substreams, and
    /// send back various events.
    fn shutdown(&mut self);

    /// Should behave like `Stream::poll()`. Should close if no more event can be produced and the
    /// node should be closed.
    fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>>, IoError>;
}

/// Endpoint for a received substream.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NodeHandlerEndpoint<TOutboundOpenInfo> {
    Dialer(TOutboundOpenInfo),
    Listener,
}

impl<TOutboundOpenInfo> NodeHandlerEndpoint<TOutboundOpenInfo> {
    /// Returns true for `Dialer`.
    #[inline]
    pub fn is_dialer(&self) -> bool {
        match self {
            NodeHandlerEndpoint::Dialer(_) => true,
            NodeHandlerEndpoint::Listener => false,
        }
    }

    /// Returns true for `Listener`.
    #[inline]
    pub fn is_listener(&self) -> bool {
        match self {
            NodeHandlerEndpoint::Dialer(_) => false,
            NodeHandlerEndpoint::Listener => true,
        }
    }
}

/// Event produced by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NodeHandlerEvent<TOutboundOpenInfo, TCustom> {
    /// Require a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest(TOutboundOpenInfo),

    /// Other event.
    Custom(TCustom),
}

/// Event produced by a handler.
impl<TOutboundOpenInfo, TCustom> NodeHandlerEvent<TOutboundOpenInfo, TCustom> {
    /// If this is `OutboundSubstreamRequest`, maps the content to something else.
    #[inline]
    pub fn map_outbound_open_info<F, I>(self, map: F) -> NodeHandlerEvent<I, TCustom>
    where F: FnOnce(TOutboundOpenInfo) -> I
    {
        match self {
            NodeHandlerEvent::OutboundSubstreamRequest(val) => {
                NodeHandlerEvent::OutboundSubstreamRequest(map(val))
            },
            NodeHandlerEvent::Custom(val) => NodeHandlerEvent::Custom(val),
        }
    }

    /// If this is `Custom`, maps the content to something else.
    #[inline]
    pub fn map_custom<F, I>(self, map: F) -> NodeHandlerEvent<TOutboundOpenInfo, I>
    where F: FnOnce(TCustom) -> I
    {
        match self {
            NodeHandlerEvent::OutboundSubstreamRequest(val) => {
                NodeHandlerEvent::OutboundSubstreamRequest(val)
            },
            NodeHandlerEvent::Custom(val) => NodeHandlerEvent::Custom(map(val)),
        }
    }
}

/// A node combined with an implementation of `NodeHandler`.
// TODO: impl Debug
pub struct HandledNode<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Node that handles the muxing.
    node: Fuse<NodeStream<TMuxer, THandler::OutboundOpenInfo>>,
    /// Handler that processes substreams.
    handler: THandler,
    /// If true, `handler` has returned `Ready(None)` and therefore shouldn't be polled again.
    handler_is_done: bool,
    // True, if the node is shutting down.
    is_shutting_down: bool
}

impl<TMuxer, THandler> HandledNode<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Builds a new `HandledNode`.
    #[inline]
    pub fn new(muxer: TMuxer, handler: THandler) -> Self {
        HandledNode {
            node: NodeStream::new(muxer).fuse(),
            handler,
            handler_is_done: false,
            is_shutting_down: false
        }
    }

    /// Injects an event to the handler.
    #[inline]
    pub fn inject_event(&mut self, event: THandler::InEvent) {
        self.handler.inject_event(event);
    }

    /// Returns true if the inbound channel of the muxer is open.
    ///
    /// If `true` is returned, more inbound substream will be received.
    #[inline]
    pub fn is_inbound_open(&self) -> bool {
        self.node.get_ref().is_inbound_open()
    }

    /// Returns true if the outbound channel of the muxer is open.
    ///
    /// If `true` is returned, more outbound substream will be opened.
    #[inline]
    pub fn is_outbound_open(&self) -> bool {
        self.node.get_ref().is_outbound_open()
    }

    /// Returns true if the handled node is in the process of shutting down.
    #[inline]
    pub fn is_shutting_down(&self) -> bool {
        self.is_shutting_down
    }

    /// Indicates to the handled node that it should shut down. After calling this method, the
    /// `Stream` will end in the not-so-distant future.
    ///
    /// After this method returns, `is_shutting_down()` should return true.
    pub fn shutdown(&mut self) {
        self.node.get_mut().shutdown_all();
        for user_data in self.node.get_mut().cancel_outgoing() {
            self.handler.inject_outbound_closed(user_data);
        }
        self.handler.shutdown();
        self.is_shutting_down = true;
    }
}

impl<TMuxer, THandler> Stream for HandledNode<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>>,
{
    type Item = THandler::OutEvent;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            if self.node.is_done() && self.handler_is_done {
                return Ok(Async::Ready(None));
            }

            let mut node_not_ready = false;

            match self.node.poll()? {
                Async::NotReady => node_not_ready = true,
                Async::Ready(Some(NodeEvent::InboundSubstream { substream })) => {
                    self.handler.inject_substream(substream, NodeHandlerEndpoint::Listener)
                }
                Async::Ready(Some(NodeEvent::OutboundSubstream { user_data, substream })) => {
                    let endpoint = NodeHandlerEndpoint::Dialer(user_data);
                    self.handler.inject_substream(substream, endpoint)
                }
                Async::Ready(None) => {
                    if !self.is_shutting_down {
                        self.is_shutting_down = true;
                        self.handler.shutdown()
                    }
                }
                Async::Ready(Some(NodeEvent::OutboundClosed { user_data })) => {
                    self.handler.inject_outbound_closed(user_data)
                }
                Async::Ready(Some(NodeEvent::InboundClosed)) => {
                    self.handler.inject_inbound_closed()
                }
            }

            match if self.handler_is_done { Async::Ready(None) } else { self.handler.poll()? } {
                Async::NotReady => {
                    if node_not_ready {
                        break
                    }
                }
                Async::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(user_data))) => {
                    if self.node.get_ref().is_outbound_open() {
                        match self.node.get_mut().open_substream(user_data) {
                            Ok(()) => (),
                            Err(user_data) => self.handler.inject_outbound_closed(user_data),
                        }
                    } else {
                        self.handler.inject_outbound_closed(user_data);
                    }
                }
                Async::Ready(Some(NodeHandlerEvent::Custom(event))) => {
                    return Ok(Async::Ready(Some(event)));
                }
                Async::Ready(None) => {
                    self.handler_is_done = true;
                    if !self.is_shutting_down {
                        self.is_shutting_down = true;
                        self.node.get_mut().cancel_outgoing();
                        self.node.get_mut().shutdown_all();
                    }
                }
            }
        }

        Ok(Async::NotReady)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use muxing::{StreamMuxer, Shutdown};
    use std::marker::PhantomData;
    use tokio::runtime::current_thread;

    // TODO: move somewhere? this could be useful as a dummy
    struct InstaCloseMuxer;
    impl StreamMuxer for InstaCloseMuxer {
        type Substream = ();
        type OutboundSubstream = ();
        fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError> { Ok(Async::Ready(None)) }
        fn open_outbound(&self) -> Self::OutboundSubstream { () }
        fn poll_outbound(&self, _: &mut Self::OutboundSubstream) -> Poll<Option<Self::Substream>, IoError> { Ok(Async::Ready(None)) }
        fn destroy_outbound(&self, _: Self::OutboundSubstream) {}
        fn read_substream(&self, _: &mut Self::Substream, _: &mut [u8]) -> Poll<usize, IoError> { panic!() }
        fn write_substream(&self, _: &mut Self::Substream, _: &[u8]) -> Poll<usize, IoError> { panic!() }
        fn flush_substream(&self, _: &mut Self::Substream) -> Poll<(), IoError> { panic!() }
        fn shutdown_substream(&self, _: &mut Self::Substream, _: Shutdown) -> Poll<(), IoError> { panic!() }
        fn destroy_substream(&self, _: Self::Substream) { panic!() }
        fn shutdown(&self, _: Shutdown) -> Poll<(), IoError> { Ok(Async::Ready(())) }
        fn flush_all(&self) -> Poll<(), IoError> { Ok(Async::Ready(())) }
    }

    #[test]
    fn proper_shutdown() {
        // Test that `shutdown()` is properly called on the handler once a node stops.
        struct Handler<T> {
            did_substream_attempt: bool,
            inbound_closed: bool,
            substream_attempt_cancelled: bool,
            shutdown_called: bool,
            marker: PhantomData<T>,
        };
        impl<T> NodeHandler for Handler<T> {
            type InEvent = ();
            type OutEvent = ();
            type Substream = T;
            type OutboundOpenInfo = ();
            fn inject_substream(&mut self, _: T, _: NodeHandlerEndpoint<()>) { panic!() }
            fn inject_inbound_closed(&mut self) {
                assert!(!self.inbound_closed);
                self.inbound_closed = true;
            }
            fn inject_outbound_closed(&mut self, _: ()) {
                assert!(!self.substream_attempt_cancelled);
                self.substream_attempt_cancelled = true;
            }
            fn inject_event(&mut self, _: Self::InEvent) { panic!() }
            fn shutdown(&mut self) {
                assert!(self.inbound_closed);
                assert!(self.substream_attempt_cancelled);
                self.shutdown_called = true;
            }
            fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<(), ()>>, IoError> {
                if self.shutdown_called {
                    Ok(Async::Ready(None))
                } else if !self.did_substream_attempt {
                    self.did_substream_attempt = true;
                    Ok(Async::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(()))))
                } else {
                    Ok(Async::NotReady)
                }
            }
        }
        impl<T> Drop for Handler<T> {
            fn drop(&mut self) {
                assert!(self.shutdown_called);
            }
        }

        let handled = HandledNode::new(InstaCloseMuxer, Handler {
            did_substream_attempt: false,
            inbound_closed: false,
            substream_attempt_cancelled: false,
            shutdown_called: false,
            marker: PhantomData,
        });

        current_thread::Runtime::new().unwrap().block_on(handled.for_each(|_| Ok(()))).unwrap();
    }
}
