// Copyright 2019 Parity Technologies (UK) Ltd.
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

use libp2p_core::{muxing, upgrade, Transport, transport::ListenerEvent};
use libp2p_tcp::TcpConfig;
use futures::prelude::*;
use std::sync::{Arc, mpsc};
use std::thread;
use tokio::runtime::current_thread::Runtime;

#[test]
fn async_write() {
    // Tests that `AsyncWrite::shutdown` implies flush.

    let (tx, rx) = mpsc::channel();

    let bg_thread = thread::spawn(move || {
        let mplex = libp2p_mplex::MplexConfig::new();

        let transport = TcpConfig::new().and_then(move |c, e|
            upgrade::apply(c, mplex, e, upgrade::Version::V1));

        let mut listener = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let addr = listener.by_ref().wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        tx.send(addr).unwrap();

        let future = listener
            .filter_map(ListenerEvent::into_upgrade)
            .into_future()
            .map_err(|(err, _)| panic!("{:?}", err))
            .and_then(|(client, _)| client.unwrap().0)
            .map_err(|err| panic!("{:?}", err))
            .and_then(|client| muxing::outbound_from_ref_and_wrap(Arc::new(client)))
            .and_then(|client| {
                tokio::io::read_to_end(client, vec![])
            })
            .and_then(|(_, msg)| {
                assert_eq!(msg, b"hello world");
                Ok(())
            });

        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
    });

    let mplex = libp2p_mplex::MplexConfig::new();
    let transport = TcpConfig::new().and_then(move |c, e|
        upgrade::apply(c, mplex, e, upgrade::Version::V1));

    let future = transport
        .dial(rx.recv().unwrap())
        .unwrap()
        .map_err(|err| panic!("{:?}", err))
        .and_then(|client| muxing::inbound_from_ref_and_wrap(Arc::new(client)))
        .and_then(|server| tokio::io::write_all(server, b"hello world"))
        .and_then(|(server, _)| {
            tokio::io::shutdown(server)
        })
        .map(|_| ());

    let mut rt = Runtime::new().unwrap();
    let _ = rt.block_on(future).unwrap();
    bg_thread.join().unwrap();
}
