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

use futures::prelude::*;
use nodes::handled_node::{NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent, HandledNode};
use super::dummy_muxer::DummyMuxer;
use muxing::SubstreamRef;
use std::sync::Arc;

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Handler {
	pub events: Vec<InEvent>,
	pub state: Option<HandlerState>,
	pub next_outbound_state: Option<HandlerState>,
}

impl Default for Handler {
	fn default() -> Self {
		Handler {
			events: Vec::new(),
			state: None,
			next_outbound_state: None,
		}
	}
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum HandlerState {
	NotReady,
	Ready(Option<NodeHandlerEvent<usize, OutEvent>>),
	Err,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum InEvent {
	Custom(&'static str),
	Substream(Option<usize>),
	OutboundClosed,
	InboundClosed,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum OutEvent {
	Custom(&'static str),
}

// Concrete `HandledNode`
pub(crate) type TestHandledNode = HandledNode<DummyMuxer, Handler>;

impl NodeHandler for Handler {
	type InEvent = InEvent;
	type OutEvent = OutEvent;
	type OutboundOpenInfo = usize;
	type Substream = SubstreamRef<Arc<DummyMuxer>>;
	fn inject_substream(&mut self, _: Self::Substream, endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>) {
		let user_data = match endpoint {
			NodeHandlerEndpoint::Dialer(user_data) => Some(user_data),
			NodeHandlerEndpoint::Listener => None
		};
		self.events.push(InEvent::Substream(user_data));
	}
	fn inject_inbound_closed(&mut self) {
		self.events.push(InEvent::InboundClosed);
	}
	fn inject_outbound_closed(&mut self, _: usize) {
		self.events.push(InEvent::OutboundClosed);
		if let Some(ref state) = self.next_outbound_state {
			self.state = Some(state.clone());
		}
	}
	fn inject_event(&mut self, inevent: Self::InEvent) {
		println!("[NodeHandler, inject_event] inevent={:?}", inevent);
		self.events.push(inevent.clone());
		match inevent {
			InEvent::Custom(s) => self.state = Some(HandlerState::Ready(Some(NodeHandlerEvent::Custom(OutEvent::Custom(s))))),
			InEvent::Substream(Some(user_data)) => {
				println!("[NodeHandler, inject_event] opening a substream with user_data={:?}", user_data);
				self.state = Some(HandlerState::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(user_data))))
			}
			_ => {}
		}

	}
	fn shutdown(&mut self) {
		println!("[NodeHandler, shutdown] Handler shutting down");
		self.state = Some(HandlerState::Ready(None));
	}
	fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<usize, OutEvent>>, IoError> {
		println!("[NodeHandler, poll] current handler state: {:?}", self.state);
		// Should ideally be `self.state.take()` but it is sometimes useful to
		// make the state "stick" so it's returned many times (it's complicated
		// to set the state from the outside once the task runs).
		match self.state {
			Some(ref state) => match state {
				HandlerState::NotReady => Ok(Async::NotReady),
				HandlerState::Ready(None) => Ok(Async::Ready(None)),
				HandlerState::Ready(Some(event)) => Ok(Async::Ready(Some(event.clone()))),
				HandlerState::Err => {Err(io::Error::new(io::ErrorKind::Other, "oh noes"))},
			},
			None => Ok(Async::NotReady)
		}
	}
}
