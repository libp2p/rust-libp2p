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

extern crate env_logger;

use std::{io, marker::PhantomData};
use futures::prelude::*;
use libp2p::core::nodes::handled_node::{NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent};
use libp2p::tokio_io::{AsyncRead, AsyncWrite};
use network;
use tokio_codec::{Framed, LinesCodec};

struct Handler<TSubstream> {
    substreams: Vec<Box<Future<Item = (), Error = io::Error> + Send>>,
    substreams_to_request: usize,
    substreams_to_finish: usize,
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Default for Handler<TSubstream> {
    #[inline]
    fn default() -> Self {
        Handler {
            substreams: Vec::new(),
            substreams_to_request: 10,
            substreams_to_finish: 20,
            marker: PhantomData,
        }
    }
}

impl<TSubstream> NodeHandler for Handler<TSubstream>
where TSubstream: AsyncRead + AsyncWrite + Send + 'static,
{
    type InEvent = ();
    type OutEvent = ();
    type OutboundOpenInfo = ();
    type Substream = TSubstream;

    fn inject_substream(&mut self, substream: TSubstream, endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>) {
        const DATA: &'static str = "hello world";

        if let NodeHandlerEndpoint::Dialer(_) = endpoint {
            assert!(self.substreams_to_finish >= 1);
        }

        let future = Framed::new(substream, LinesCodec::new())
            .send(DATA.to_string())
            .and_then(|stream| {
                stream.into_future().map(|(out, _)| out).map_err(|(err, _)| err)
            })
            .map(|out| assert_eq!(&out.expect("Didn't receive message back"), DATA));

        self.substreams.push(Box::new(future));
    }

    fn inject_inbound_closed(&mut self) {
    }

    fn inject_outbound_closed(&mut self, _: Self::OutboundOpenInfo) {
        panic!()
    }

    fn inject_event(&mut self, _: Self::InEvent) {
        panic!()
    }

    fn shutdown(&mut self) {
        assert_eq!(self.substreams_to_request, 0);
        assert_eq!(self.substreams_to_finish, 0);

        // TODO: not sure if shutdown() is supposed to be called
        //assert!(self.substreams.is_empty());
    }

    fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>>, io::Error> {
        if self.substreams_to_request >= 1 {
            self.substreams_to_request -= 1;
            return Ok(Async::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(()))));
        }

        for n in (0..self.substreams.len()).rev() {
            let mut substream = self.substreams.swap_remove(n);
            match substream.poll()? {
                Async::NotReady => self.substreams.push(substream),
                Async::Ready(()) => self.substreams_to_finish -= 1,
            }
        }

        if self.substreams_to_finish == 0 && self.substreams.is_empty() {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

#[test]
fn basic_flood() {
    env_logger::init();

    let mut network = network::Network::default();

    let mut node_ids = Vec::new();
    for n in 0 .. 25 {
        let id = network.node(format!("node{}", n), || Handler::default());
        node_ids.push(id);
    }

    network.connect_all(&node_ids);
    network.start();
}
