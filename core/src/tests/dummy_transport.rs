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

//! `DummyListener` is a `Listener` used in tests. It implements a bare-bones
//! version of the trait along with a way to setup the transport listeners with
//! an initial state to facilitate testing.

use crate::{Multiaddr, Listener};
use futures::prelude::*;
use futures::{
    future::{self, FutureResult},
    stream,
};
use std::io;

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum ListenerState {
    /// The `usize` indexes items produced by the listener
    Ok(Async<Option<usize>>),
    Error,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) struct DummyListener {
    state: ListenerState,
}

impl DummyListener {
    pub(crate) fn new() -> Self {
        DummyListener {
            state: ListenerState::Ok(Async::NotReady)
        }
    }

    pub(crate) fn set_initial_listener_state(&mut self, state: ListenerState) {
        self.state = state;
    }
}

impl Listener for DummyListener {
    type Output = usize;
    type Error = io::Error;
    type Inbound = Box<Stream<Item=(Self::Upgrade, Multiaddr), Error=Self::Error> + Send>;
    type Upgrade = FutureResult<Self::Output, Self::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        let addr2 = addr.clone();
        match self.state {
            ListenerState::Ok(async) => {
                let tupelize = move |stream| (future::ok(stream), addr.clone());
                Ok(match async {
                    Async::NotReady => {
                        let stream = stream::poll_fn(|| Ok(Async::NotReady)).map(tupelize);
                        (Box::new(stream), addr2)
                    }
                    Async::Ready(Some(n)) => {
                        let stream = stream::iter_ok(n..).map(tupelize);
                        (Box::new(stream), addr2)
                    }
                    Async::Ready(None) => {
                        let stream = stream::empty();
                        (Box::new(stream), addr2)
                    }
                })
            }
            ListenerState::Error => Err((self, addr2)),
        }
    }

    fn nat_traversal(&self, _server: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        unimplemented!();
    }
}
