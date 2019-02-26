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

#![cfg(test)]

use super::*;
use assert_matches::assert_matches;
use tokio::runtime::current_thread;
use crate::tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
use crate::tests::dummy_handler::{Handler, HandlerState, InEvent, OutEvent, TestHandledNode};
use std::{io, marker::PhantomData};

struct TestBuilder {
    muxer: DummyMuxer,
    handler: Handler,
    want_open_substream: bool,
    substream_user_data: usize,
}

impl TestBuilder {
    fn new() -> Self {
        TestBuilder {
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

    fn handled_node(&mut self) -> TestHandledNode {
        let mut h = HandledNode::new(self.muxer.clone(), self.handler.clone());
        if self.want_open_substream {
            h.node.get_mut().open_substream(self.substream_user_data).expect("open substream should work");
        }
        h
    }
}

// Set the state of the `Handler` after `inject_outbound_closed` is called
fn set_next_handler_outbound_state( handled_node: &mut TestHandledNode, next_state: HandlerState) {
    handled_node.handler.next_outbound_state = Some(next_state);
}

#[test]
fn proper_shutdown() {
    struct ShutdownHandler<T> {
        did_substream_attempt: bool,
        inbound_closed: bool,
        substream_attempt_cancelled: bool,
        shutdown_called: bool,
        marker: PhantomData<T>
    }
    impl<T> NodeHandler for ShutdownHandler<T> {
        type InEvent = ();
        type OutEvent = ();
        type Substream = T;
        type Error = io::Error;
        type OutboundOpenInfo = ();
        fn inject_substream(&mut self, _: Self::Substream, _: NodeHandlerEndpoint<Self::OutboundOpenInfo>) { panic!() }
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
        fn poll(&mut self) -> Poll<NodeHandlerEvent<(), ()>, io::Error> {
            if self.shutdown_called {
                Ok(Async::Ready(NodeHandlerEvent::Shutdown))
            } else if !self.did_substream_attempt {
                self.did_substream_attempt = true;
                Ok(Async::Ready(NodeHandlerEvent::OutboundSubstreamRequest(())))
            } else {
                Ok(Async::NotReady)
            }
        }
    }

    impl<T> Drop for ShutdownHandler<T> {
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
    let handled = HandledNode::new(muxer, ShutdownHandler {
        did_substream_attempt: false,
        inbound_closed: false,
        substream_attempt_cancelled: false,
        shutdown_called: false,
        marker: PhantomData,
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
    assert_eq!(handled.handler().events, vec![event]);
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
fn is_shutting_down_is_true_when_in_and_outbounds_are_closed() {
    let mut handled = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Closed)
        .with_muxer_outbound_state(DummyConnectionState::Closed)
        .with_open_substream(123) // avoid infinite loop
        .handled_node();

    handled.poll().expect("poll should work");

    // Shutting down (in- and outbound are closed, and the handler is shutdown)
    assert!(handled.is_shutting_down());
}

#[test]
fn is_shutting_down_is_true_when_handler_is_gone() {
    // when in-/outbound NodeStreams are  open or Async::Ready(None) we reach the handlers `poll()` and initiate shutdown.
    let mut handled = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Pending)
        .with_muxer_outbound_state(DummyConnectionState::Pending)
        .with_handler_state(HandlerState::Ready(None)) // avoid infinite loop
        .handled_node();

    handled.poll().expect("poll should work");

    assert!(handled.is_shutting_down());
}

#[test]
fn is_shutting_down_is_true_when_handler_is_gone_even_if_in_and_outbounds_are_open() {
    let mut handled = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Opened)
        .with_muxer_outbound_state(DummyConnectionState::Opened)
        .with_open_substream(123)
        .with_handler_state(HandlerState::Ready(None))
        .handled_node();

    handled.poll().expect("poll should work");

    assert!(handled.is_shutting_down());
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
    handled.poll().expect("poll works");
    assert_eq!(handled.handler().events, vec![InEvent::OutboundClosed]);
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
    // - back in `poll()` we call `handler.poll()` which does nothing because
    //   `HandlerState` is `NotReady`: loop continues
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
    // - HandledNode polls the node again: we skip inbound and there are no
    //   more outbound substreams so we skip that too; the addr is now
    //   Resolved so that part is skipped too
    // - We reach the last section and the NodeStream yields Async::Ready(None)
    // - Back in HandledNode the Async::Ready(None) triggers a shutdown
    // – …and causes the Handler to yield Async::Ready(None)
    // – which in turn makes the HandledNode to yield Async::Ready(None) as well
    assert_matches!(handled.poll(), Ok(Async::Ready(None)));
    assert_eq!(handled.handler().events, vec![
        InEvent::InboundClosed, InEvent::OutboundClosed
    ]);
}

#[test]
fn poll_yields_inbound_closed_event() {
    let mut h = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Closed)
        .with_handler_state(HandlerState::Err) // stop the loop
        .handled_node();

    assert_eq!(h.handler().events, vec![]);
    let _ = h.poll();
    assert_eq!(h.handler().events, vec![InEvent::InboundClosed]);
}

#[test]
fn poll_yields_outbound_closed_event() {
    let mut h = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Pending)
        .with_open_substream(32)
        .with_muxer_outbound_state(DummyConnectionState::Closed)
        .with_handler_state(HandlerState::Err) // stop the loop
        .handled_node();

    assert_eq!(h.handler().events, vec![]);
    let _ = h.poll();
    assert_eq!(h.handler().events, vec![InEvent::OutboundClosed]);
}

#[test]
fn poll_yields_outbound_substream() {
    let mut h = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Pending)
        .with_muxer_outbound_state(DummyConnectionState::Opened)
        .with_open_substream(1)
        .with_handler_state(HandlerState::Err) // stop the loop
        .handled_node();

    assert_eq!(h.handler().events, vec![]);
    let _ = h.poll();
    assert_eq!(h.handler().events, vec![InEvent::Substream(Some(1))]);
}

#[test]
fn poll_yields_inbound_substream() {
    let mut h = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Opened)
        .with_muxer_outbound_state(DummyConnectionState::Pending)
        .with_handler_state(HandlerState::Err) // stop the loop
        .handled_node();

    assert_eq!(h.handler().events, vec![]);
    let _ = h.poll();
    assert_eq!(h.handler().events, vec![InEvent::Substream(None)]);
}
