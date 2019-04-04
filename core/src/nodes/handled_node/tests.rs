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
use crate::tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
use crate::tests::dummy_handler::{Handler, HandlerState, InEvent, OutEvent, TestHandledNode};

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
            h.node.open_substream(self.substream_user_data);
        }
        h
    }
}

// Set the state of the `Handler` after `inject_outbound_closed` is called
fn set_next_handler_outbound_state( handled_node: &mut TestHandledNode, next_state: HandlerState) {
    handled_node.handler.next_outbound_state = Some(next_state);
}

#[test]
fn can_inject_event() {
    let mut handled = TestBuilder::new()
        .handled_node();

    let event = InEvent::Custom("banana");
    handled.inject_event(event.clone());
    assert_eq!(handled.handler().events, vec![event]);
}

#[test]
fn poll_with_unready_node_stream_and_handler_emits_custom_event() {
    let expected_event = NodeHandlerEvent::Custom(OutEvent::Custom("pineapple"));
    let mut handled = TestBuilder::new()
        // make NodeStream return NotReady
        .with_muxer_inbound_state(DummyConnectionState::Pending)
        // make Handler return return Ready(Some(…))
        .with_handler_state(HandlerState::Ready(expected_event))
        .handled_node();

    assert_matches!(handled.poll(), Ok(Async::Ready(event)) => {
        assert_matches!(event, OutEvent::Custom("pineapple"))
    });
}

#[test]
fn handler_emits_outbound_closed_when_opening_new_substream_on_closed_node() {
    let open_event = NodeHandlerEvent::OutboundSubstreamRequest(456);
    let mut handled = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Pending)
        .with_muxer_outbound_state(DummyConnectionState::Pending)
        .with_handler_state(HandlerState::Ready(open_event))
        .handled_node();

    set_next_handler_outbound_state(
        &mut handled,
        HandlerState::Ready(NodeHandlerEvent::Custom(OutEvent::Custom("pear")))
    );
    handled.poll().expect("poll works");
}

#[test]
fn poll_yields_inbound_closed_event() {
    let mut h = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Pending)
        .with_handler_state(HandlerState::Err) // stop the loop
        .handled_node();

    assert_eq!(h.handler().events, vec![]);
    let _ = h.poll();
}

#[test]
fn poll_yields_outbound_closed_event() {
    let mut h = TestBuilder::new()
        .with_muxer_inbound_state(DummyConnectionState::Pending)
        .with_open_substream(32)
        .with_muxer_outbound_state(DummyConnectionState::Pending)
        .with_handler_state(HandlerState::Err) // stop the loop
        .handled_node();

    assert_eq!(h.handler().events, vec![]);
    let _ = h.poll();
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
