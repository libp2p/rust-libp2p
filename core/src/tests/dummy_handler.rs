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

//! Concrete `NodeHandler` implementation and assorted testing types

use std::io::{self, Error as IoError};

use super::dummy_muxer::DummyMuxer;
use futures::prelude::*;
use crate::muxing::SubstreamRef;
use crate::nodes::handled_node::{HandledNode, NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent};
use std::sync::Arc;

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Handler {
    /// Inspect events passed through the Handler
    pub events: Vec<InEvent>,
    /// Current state of the Handler
    pub state: Option<HandlerState>,
    /// Next state for outbound streams of the Handler
    pub next_outbound_state: Option<HandlerState>,
    /// Vec of states the Handler will assume
    pub next_states: Vec<HandlerState>,
}

impl Default for Handler {
    fn default() -> Self {
        Handler {
            events: Vec::new(),
            state: None,
            next_states: Vec::new(),
            next_outbound_state: None,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum HandlerState {
    Ready(NodeHandlerEvent<usize, OutEvent>),
    Err,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum InEvent {
    /// A custom inbound event
    Custom(&'static str),
    /// A substream request with a dummy payload
    Substream(Option<usize>),
    /// Request the handler to move to the next state
    NextState,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum OutEvent {
    /// A message from the Handler upwards in the stack
    Custom(&'static str),
}

// Concrete `HandledNode` parametrised for the test helpers
pub(crate) type TestHandledNode = HandledNode<DummyMuxer, Handler>;

impl NodeHandler for Handler {
    type InEvent = InEvent;
    type OutEvent = OutEvent;
    type Error = IoError;
    type OutboundOpenInfo = usize;
    type Substream = SubstreamRef<Arc<DummyMuxer>>;
    fn inject_substream(
        &mut self,
        _: Self::Substream,
        endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        let user_data = match endpoint {
            NodeHandlerEndpoint::Dialer(user_data) => Some(user_data),
            NodeHandlerEndpoint::Listener => None,
        };
        self.events.push(InEvent::Substream(user_data));
    }
    fn inject_event(&mut self, inevent: Self::InEvent) {
        self.events.push(inevent.clone());
        match inevent {
            InEvent::Custom(s) => {
                self.state = Some(HandlerState::Ready(NodeHandlerEvent::Custom(
                    OutEvent::Custom(s),
                )))
            }
            InEvent::Substream(Some(user_data)) => {
                self.state = Some(HandlerState::Ready(
                    NodeHandlerEvent::OutboundSubstreamRequest(user_data),
                ))
            }
            InEvent::NextState => {
                let next_state = self.next_states.pop();
                self.state = next_state
            }
            _ => unreachable!(),
        }
    }
    fn poll(&mut self) -> Poll<NodeHandlerEvent<usize, OutEvent>, IoError> {
        match self.state.take() {
            Some(ref state) => match state {
                HandlerState::Ready(event) => Ok(Async::Ready(event.clone())),
                HandlerState::Err => Err(io::Error::new(io::ErrorKind::Other, "oh noes")),
            },
            None => Ok(Async::NotReady),
        }
    }
}
