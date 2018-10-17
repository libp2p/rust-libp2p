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
use Multiaddr;

/// Handler for the substreams of a node.
///
/// > Note: When implementing the various methods, don't forget that you have to register the
/// > task that was the latest to poll and notify it.
// TODO: right now it is possible for a node handler to be built, then shut down right after if we
//       realize we dialed the wrong peer for example ; this could be surprising and should either
//       be documented or changed (favouring the "documented" right now)
pub trait NodeHandler<TSubstream> {
    /// Custom event that can be received from the outside.
    type InEvent;
    /// Custom event that can be produced by the handler and that will be returned by the swarm.
    type OutEvent;
    /// Information about a substream. Can be sent to the handler through a `NodeHandlerEndpoint`,
    /// and will be passed back in `inject_substream` or `inject_outbound_closed`.
    type OutboundOpenInfo;

    /// Sends a new substream to the handler.
    ///
    /// The handler is responsible for upgrading the substream to whatever protocol it wants.
    fn inject_substream(&mut self, substream: TSubstream, endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>);

    /// Indicates to the handler that the inbound part of the muxer has been closed, and that
    /// therefore no more inbound substream will be produced.
    fn inject_inbound_closed(&mut self);

    /// Indicates to the handler that an outbound substream failed to open because the outbound
    /// part of the muxer has been closed.
    fn inject_outbound_closed(&mut self, user_data: Self::OutboundOpenInfo);

    /// Indicates to the handler that the multiaddr future has resolved.
    fn inject_multiaddr(&mut self, multiaddr: Result<Multiaddr, IoError>);

    /// Injects an event coming from the outside into the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Indicates to the node that it should shut down. After that, it is expected that `poll()`
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
pub struct HandledNode<TMuxer, TAddrFut, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
{
    /// Node that handles the muxing.
    node: Fuse<NodeStream<TMuxer, TAddrFut, THandler::OutboundOpenInfo>>,
    /// Handler that processes substreams.
    handler: THandler,
    // True, if the node is shutting down.
    is_shutting_down: bool
}

impl<TMuxer, TAddrFut, THandler> HandledNode<TMuxer, TAddrFut, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
{
    /// Builds a new `HandledNode`.
    #[inline]
    pub fn new(muxer: TMuxer, multiaddr_future: TAddrFut, handler: THandler) -> Self {
        HandledNode {
            node: NodeStream::new(muxer, multiaddr_future).fuse(),
            handler,
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
        self.is_shutting_down = true;

        for user_data in self.node.get_mut().cancel_outgoing() {
            self.handler.inject_outbound_closed(user_data);
        }

        self.handler.shutdown()
    }
}

impl<TMuxer, TAddrFut, THandler> Stream for HandledNode<TMuxer, TAddrFut, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
{
    type Item = THandler::OutEvent;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        println!("[HandledNode, poll] START");
        loop {
            println!("[HandledNode, poll] top of the loop");
            let mut node_not_ready = false;

            println!("[HandledNode, poll]   node");
            match self.node.poll()? {
                Async::NotReady => {
                    println!("[HandledNode, poll]   node; Async::NotReady");
                    ()
                },
                Async::Ready(Some(NodeEvent::InboundSubstream { substream })) => {
                    println!("[HandledNode, poll]   node; Async::Ready(Some(InboundStream)");
                    self.handler.inject_substream(substream, NodeHandlerEndpoint::Listener)
                }
                Async::Ready(Some(NodeEvent::OutboundSubstream { user_data, substream })) => {
                    println!("[HandledNode, poll]   node; Async::Ready(Some(OutboundStream)");
                    let endpoint = NodeHandlerEndpoint::Dialer(user_data);
                    self.handler.inject_substream(substream, endpoint)
                }
                Async::Ready(None) => {
                    println!("[HandledNode, poll]   node; Async::Ready(None) – are we shutting down? {:?}", self.is_shutting_down);
                    node_not_ready = true;
                    if !self.is_shutting_down {
                        println!("[HandledNode, poll]   node; Async::Ready(None) – are we shutting down? No. Starting shutdown.");
                        self.handler.shutdown()
                    }
                }
                Async::Ready(Some(NodeEvent::Multiaddr(result))) => {
                    println!("[HandledNode, poll]   node; Async::Ready(Some(Multiaddr))");
                    self.handler.inject_multiaddr(result)
                }
                Async::Ready(Some(NodeEvent::OutboundClosed { user_data })) => {
                    println!("[HandledNode, poll]   node; Async::Ready(Some(OutboundClosed))");
                    self.handler.inject_outbound_closed(user_data)
                }
                Async::Ready(Some(NodeEvent::InboundClosed)) => {
                    println!("[HandledNode, poll]   node; Async::Ready(Some(InboundClosed))");
                    self.handler.inject_inbound_closed()
                }
            }
            println!("[HandledNode, poll] handler");
            match self.handler.poll()? {
                Async::NotReady => {
                    println!("[HandledNode, poll]   handler; Async::NotReady");
                    if node_not_ready {
                        break
                    }
                }
                Async::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(user_data))) => {
                    println!("[HandledNode, poll]   handler; Async::Ready(Some(OutboundSubstreamRequest))");
                    if self.node.get_ref().is_outbound_open() {
                        println!("[HandledNode, poll]       handler; outbound is open");
                        match self.node.get_mut().open_substream(user_data) {
                            Ok(()) => {
                                println!("[HandledNode, poll]           handler; open_substream ok");
                                ()
                            },
                            Err(user_data) => {
                                println!("[HandledNode, poll]           handler; open_substream failed");
                                self.handler.inject_outbound_closed(user_data)
                            },
                        }
                    } else {
                        println!("[HandledNode, poll]       handler; outbound is closed");
                        self.handler.inject_outbound_closed(user_data);
                    }
                }
                Async::Ready(Some(NodeHandlerEvent::Custom(event))) => {
                    println!("[HandledNode, poll]     handler; Async::Ready(Some(Custom))");
                    return Ok(Async::Ready(Some(event)));
                }
                Async::Ready(None) => {
                    println!("[HandledNode, poll]     handler; Async::Ready(None)");
                    return Ok(Async::Ready(None))
                }
            }
        }
        println!("[HandledNode, poll] NotReady");
        Ok(Async::NotReady)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future;
    use futures::future::FutureResult;
    use tokio::runtime::current_thread;
    use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
    use tests::dummy_handler::{Handler, HandlerState, InEvent, OutEvent};

    // Concrete `HandledNode`
    type TestHandledNode = HandledNode<DummyMuxer, FutureResult<Multiaddr, IoError>, Handler>;

    struct TestBuilder {
        addr: Multiaddr,
        muxer: DummyMuxer,
        handler: Handler,
        want_open_substream: bool,
        substream_user_data: usize,
    }

    impl TestBuilder {
        fn new() -> Self {
            TestBuilder {
                addr: "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"),
                muxer: DummyMuxer::new(),
                handler: Handler::default(),
                want_open_substream: false,
                substream_user_data: 0,
            }
        }

        fn with_muxer_inbound_state(&mut self, state: DummyConnectionState) -> &mut Self {
            self.muxer.set_inbound_connection_state(state);
            self
        }

        fn with_muxer_outbound_state(&mut self, state: DummyConnectionState) -> &mut Self {
            self.muxer.set_outbound_connection_state(state);
            self
        }

        fn with_handler_state(&mut self, state: HandlerState) -> &mut Self {
            self.handler.state = Some(state);
            self
        }

        fn with_open_substream(&mut self, user_data: usize) -> &mut Self {
            self.want_open_substream = true;
            self.substream_user_data = user_data;
            self
        }

        // TODO: Is there a way to consume `self` here and get rid of the clones?
        fn handled_node(&mut self) -> TestHandledNode {
            let mut h = HandledNode::new(self.muxer.clone(), future::ok(self.addr.clone()), self.handler.clone());
            if self.want_open_substream {
                h.node.get_mut().open_substream(self.substream_user_data).expect("open substream should work");
            }
            h
        }
    }

    fn did_see_event(handled_node: &mut TestHandledNode, event: &InEvent) -> bool {
        handled_node.handler.events.contains(event)
    }

    // Set the state of the `Handler` after `inject_outbound_closed` is called
    fn set_next_handler_outbound_state( handled_node: &mut TestHandledNode, next_state: HandlerState) {
        handled_node.handler.next_outbound_state = Some(next_state);
    }

    #[test]
    fn proper_shutdown() {
        struct ShutdownHandler {
            did_substream_attempt: bool,
            inbound_closed: bool,
            substream_attempt_cancelled: bool,
            shutdown_called: bool,
        }
        impl<T> NodeHandler<T> for ShutdownHandler {
            type InEvent = ();
            type OutEvent = ();
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
            fn inject_multiaddr(&mut self, _: Result<Multiaddr, IoError>) {}
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

        impl Drop for ShutdownHandler {
            fn drop(&mut self) {
                if self.did_substream_attempt {
                    assert!(self.shutdown_called);
                }
            }
        }

        // Test that `shutdown()` is properly called on the handler once a node stops.
        let mut muxer = DummyMuxer::new();
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let handled = HandledNode::new(muxer, future::empty(), ShutdownHandler {
            did_substream_attempt: false,
            inbound_closed: false,
            substream_attempt_cancelled: false,
            shutdown_called: false,
        });

        current_thread::Runtime::new().unwrap().block_on(handled.for_each(|_| Ok(()))).unwrap();
    }

    #[test]
    fn can_inject_event() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
            .handled_node();

        let event = InEvent::Custom("banana");
        handled.inject_event(event.clone());
        assert!(did_see_event(&mut handled, &event));
    }

    #[test]
    fn knows_if_inbound_is_closed() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
            .with_handler_state(HandlerState::Ready(None)) // or we get into an infinite loop
            .handled_node();
        handled.poll().expect("poll failed");
        assert!(!handled.is_inbound_open())
    }

    #[test]
    fn knows_if_outbound_is_closed() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_muxer_outbound_state(DummyConnectionState::Closed)
            .with_handler_state(HandlerState::Ready(None)) // or we get into an infinite loop
            .with_open_substream(987) // without at least one substream we do not poll_outbound so we never get the event
            .handled_node();

        handled.poll().expect("poll failed");
        assert!(!handled.is_outbound_open());
    }

    #[test]
    fn is_shutting_down_is_true_when_called_shutdown_on_the_handled_node() {
        let mut handled = TestBuilder::new()
            .with_handler_state(HandlerState::Ready(None)) // Stop the loop towards the end of the first run
            .handled_node();
        assert!(!handled.is_shutting_down());
        handled.poll().expect("poll should work");
        handled.shutdown();
        assert!(handled.is_shutting_down());
    }

    #[test]
    fn is_shutting_down_is_false_even_when_in_and_outbounds_are_closed() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
            .with_muxer_outbound_state(DummyConnectionState::Closed)
            .with_open_substream(123) // avoid infinite loop
            .handled_node();

        handled.poll().expect("poll failed");

        // Not shutting down (but in- and outbound are closed, and the handler is shutdown)
        assert!(!handled.is_shutting_down());

        // when in-/outbound NodeStreams are  open or Async::Ready(None) we reach the handler `poll()`
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_muxer_outbound_state(DummyConnectionState::Pending)
            .with_handler_state(HandlerState::Ready(None)) // avoid infinite loop
            .handled_node();

        handled.poll().expect("poll failed");

        // still open for business
        assert!(!handled.is_shutting_down());
    }

    #[test]
    fn poll_with_unready_node_stream_polls_handler() {
        let mut handled = TestBuilder::new()
            // make NodeStream return NotReady
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            // make Handler return return Ready(None) so we break the infinite loop
            .with_handler_state(HandlerState::Ready(None))
            .handled_node();

        assert_matches!(handled.poll(), Ok(Async::Ready(None)));
        assert_eq!(handled.handler.events, vec![InEvent::Multiaddr]);
    }

    #[test]
    fn poll_with_unready_node_stream_and_handler_emits_custom_event() {
        let expected_event = Some(NodeHandlerEvent::Custom(OutEvent::Custom("pineapple")));
        let mut handled = TestBuilder::new()
            // make NodeStream return NotReady
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            // make Handler return return Ready(Some(…))
            .with_handler_state(HandlerState::Ready(expected_event))
            .handled_node();

        assert_matches!(handled.poll(), Ok(Async::Ready(Some(event))) => {
            assert_matches!(event, OutEvent::Custom("pineapple"))
        });
        assert_eq!(handled.handler.events, vec![InEvent::Multiaddr]);
    }

    #[test]
    fn handler_emits_outbound_closed_when_opening_new_substream_on_closed_node() {
        let open_event = Some(NodeHandlerEvent::OutboundSubstreamRequest(456));
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_muxer_outbound_state(DummyConnectionState::Closed)
            .with_handler_state(HandlerState::Ready(open_event))
            .handled_node();

        set_next_handler_outbound_state(
            &mut handled,
            HandlerState::Ready(Some(NodeHandlerEvent::Custom(OutEvent::Custom("pear"))))
        );
        handled.poll();
        assert_eq!(handled.handler.events, vec![InEvent::Multiaddr, InEvent::OutboundClosed]);
    }

    #[test]
    fn poll_returns_not_ready_when_node_stream_and_handler_is_not_ready() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
            .with_muxer_outbound_state(DummyConnectionState::Closed)
            .with_open_substream(12)
            .with_handler_state(HandlerState::NotReady)
            .handled_node();

        // Under the hood, this is what happens when calling `poll()`:
        // - we reach `node.poll_inbound()` and because the connection is
        //   closed, `inbound_finished` is set to true.
        // - an Async::Ready(NodeEvent::InboundClosed) is yielded (also calls
        //   `inject_inbound_close`, but that's irrelevant here)
        // - back in `poll()` we call `handler.poll()` which does nothing
        //   because `HandlerState` is `NotReady`: loop continues
        // - polls the node again which now skips the inbound block because
        //   `inbound_finished` is true.
        // - Now `poll_outbound()` is called which returns `Async::Ready(None)`
        //   and sets `outbound_finished` to true. …calls destroy_outbound and
        //   yields Ready(OutboundClosed) …so the HandledNode calls
        //   `inject_outbound_closed`.
        // - Now we have `inbound_finished` and `outbound_finished` set (and no
        //   more outbound substreams).
        // - Next we poll the handler again which again does nothing because
        //   HandlerState is NotReady (and the node is still there)
        // - Polls the node again; now we will hit the address resolution
        // - Address resolves and yields a `Multiaddr` event and we resume the
        //   loop
        // - HandledNode polls the node again: we skip inbound and there are no
        //   more outbound substreams so we skip that too; the addr is now
        //   Resolved so that part is skipped too
        // - We reach the last section and the NodeStream yields Async::Ready(None)
        // - Back in HandledNode the Async::Ready(None) triggers a shutdown
        // – …and causes the Handler to yield Async::Ready(None)
        // – which in turn makes the HandledNode to yield Async::Ready(None) as well
        assert_matches!(handled.poll(), Ok(Async::Ready(None)));
        assert_eq!(handled.handler.events, vec![
            InEvent::InboundClosed, InEvent::OutboundClosed, InEvent::Multiaddr
        ]);
    }

    #[test]
    fn poll_yields_inbound_closed_event() {
        let mut h = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
            .with_handler_state(HandlerState::Err) // stop the loop
            .handled_node();

        assert_eq!(h.handler.events, vec![]);
        let _ = h.poll();
        assert_eq!(h.handler.events, vec![InEvent::InboundClosed]);
    }

    #[test]
    fn poll_yields_outbound_closed_event() {
        let mut h = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_open_substream(32)
            .with_muxer_outbound_state(DummyConnectionState::Closed)
            .with_handler_state(HandlerState::Err) // stop the loop
            .handled_node();

        assert_eq!(h.handler.events, vec![]);
        let _ = h.poll();
        assert_eq!(h.handler.events, vec![InEvent::OutboundClosed]);
    }

    #[test]
    fn poll_yields_multiaddr_event() {
        let mut h = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_handler_state(HandlerState::Err) // stop the loop
            .handled_node();

        assert_eq!(h.handler.events, vec![]);
        let _ = h.poll();
        assert_eq!(h.handler.events, vec![InEvent::Multiaddr]);
    }

    #[test]
    fn poll_yields_outbound_substream() {
        let mut h = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_muxer_outbound_state(DummyConnectionState::Opened)
            .with_open_substream(1)
            .with_handler_state(HandlerState::Err) // stop the loop
            .handled_node();

        assert_eq!(h.handler.events, vec![]);
        let _ = h.poll();
        assert_eq!(h.handler.events, vec![InEvent::Substream(Some(1))]);
    }

    #[test]
    fn poll_yields_inbound_substream() {
        let mut h = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Opened)
            .with_muxer_outbound_state(DummyConnectionState::Pending)
            .with_handler_state(HandlerState::Err) // stop the loop
            .handled_node();

        assert_eq!(h.handler.events, vec![]);
        let _ = h.poll();
        assert_eq!(h.handler.events, vec![InEvent::Substream(None)]);
    }
}
