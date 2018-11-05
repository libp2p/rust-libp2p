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
use muxing::SubstreamRef;
use nodes::handled_node::{NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent};
use std::sync::Arc;

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Handler {
    pub events: Vec<Event>,
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
    Ready(Option<NodeHandlerEvent<usize, Event>>),
    Err,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum Event {
    Custom(&'static str),
    Substream(Option<usize>),
    OutboundClosed,
    InboundClosed,
}

impl NodeHandler for Handler {
    type InEvent = Event;
    type OutEvent = Event;
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
        self.events.push(Event::Substream(user_data));
    }
    fn inject_inbound_closed(&mut self) {
        self.events.push(Event::InboundClosed);
    }
    fn inject_outbound_closed(&mut self, _: usize) {
        self.events.push(Event::OutboundClosed);
        if let Some(ref state) = self.next_outbound_state {
            self.state = Some(state.clone());
        }
    }
    fn inject_event(&mut self, inevent: Self::InEvent) {
        self.events.push(inevent)
    }
    fn shutdown(&mut self) {
        self.state = Some(HandlerState::Ready(None));
    }
    fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<usize, Event>>, IoError> {
        match self.state {
            Some(ref state) => match state {
                HandlerState::NotReady => Ok(Async::NotReady),
                HandlerState::Ready(None) => Ok(Async::Ready(None)),
                HandlerState::Ready(Some(event)) => Ok(Async::Ready(Some(event.clone()))),
                HandlerState::Err => Err(io::Error::new(io::ErrorKind::Other, "oh noes")),
            },
            None => Ok(Async::NotReady),
        }
    }
}
