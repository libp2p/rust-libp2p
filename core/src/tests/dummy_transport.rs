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

//! `DummyTransport` is a `Transport` used in tests. It implements a bare-bones
//! version of the trait along with a way to setup the transport listeners with
//! an initial state to facilitate testing.

use futures::prelude::*;
use futures::{
    future::{self, FutureResult},
    stream,
};
use std::io;
use crate::{Multiaddr, PeerId, Transport, transport::{ListenerEvent, TransportError}};
use crate::tests::dummy_muxer::DummyMuxer;

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ListenerState {
    Ok(Async<Option<ListenerEvent<(PeerId, DummyMuxer)>>>),
    Error,
    Events(Vec<ListenerEvent<(PeerId, DummyMuxer)>>)
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct DummyTransport {
    /// The current state of Listeners.
    listener_state: ListenerState,
    /// The next peer returned from dial().
    next_peer_id: Option<PeerId>,
    /// When true, all dial attempts return error.
    dial_should_fail: bool,
}
impl DummyTransport {
    pub(crate) fn new() -> Self {
        DummyTransport {
            listener_state: ListenerState::Ok(Async::NotReady),
            next_peer_id: None,
            dial_should_fail: false,
        }
    }
    pub(crate) fn set_initial_listener_state(&mut self, state: ListenerState) {
        self.listener_state = state;
    }

    pub(crate) fn set_next_peer_id(&mut self, peer_id: &PeerId) {
        self.next_peer_id = Some(peer_id.clone());
    }

    pub(crate) fn make_dial_fail(&mut self) {
        self.dial_should_fail = true;
    }
}
impl Transport for DummyTransport {
    type Output = (PeerId, DummyMuxer);
    type Error = io::Error;
    type Listener = Box<dyn Stream<Item=ListenerEvent<Self::ListenerUpgrade>, Error=io::Error> + Send>;
    type ListenerUpgrade = FutureResult<Self::Output, io::Error>;
    type Dial = Box<dyn Future<Item = Self::Output, Error = io::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>>
    where
        Self: Sized,
    {
        match self.listener_state {
            ListenerState::Ok(state) => match state {
                Async::NotReady => Ok(Box::new(stream::poll_fn(|| Ok(Async::NotReady)))),
                Async::Ready(Some(event)) => Ok(Box::new(stream::poll_fn(move || {
                    Ok(Async::Ready(Some(event.clone().map(future::ok))))
                }))),
                Async::Ready(None) => Ok(Box::new(stream::empty()))
            },
            ListenerState::Error => Err(TransportError::MultiaddrNotSupported(addr)),
            ListenerState::Events(events) =>
                Ok(Box::new(stream::iter_ok(events.into_iter().map(|e| e.map(future::ok)))))
        }
    }

    fn dial(self, _addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>>
    where
        Self: Sized,
    {
        let peer_id = if let Some(peer_id) = self.next_peer_id {
            peer_id
        } else {
            PeerId::random()
        };

        let fut =
            if self.dial_should_fail {
                let err_string = format!("unreachable host error, peer={:?}", peer_id);
                future::err(io::Error::new(io::ErrorKind::Other, err_string))
            } else {
                future::ok((peer_id, DummyMuxer::new()))
            };

        Ok(Box::new(fut))
    }
}
