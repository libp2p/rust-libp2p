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

extern crate bytes;
extern crate futures;
extern crate libp2p_swarm;
extern crate libp2p_tcp_transport;
extern crate multiplex;
extern crate tokio_core;
extern crate tokio_io;

use bytes::BytesMut;
use futures::future::Future;
use futures::{Stream, Sink};
use libp2p_swarm::{Transport, StreamMuxer};
use libp2p_tcp_transport::TcpConfig;
use tokio_core::reactor::Core;
use tokio_io::codec::length_delimited::Framed;
use std::sync::mpsc;
use std::thread;

#[test]
fn client_to_server_outbound() {
    // Simulate a client sending a message to a server through a multiplex upgrade.

    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let mut core = Core::new().unwrap();
        let transport = TcpConfig::new(core.handle())
            .with_upgrade(multiplex::MultiplexConfig)
            .into_connection_reuse();

        let (listener, addr) = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap_or_else(|_| panic!());
        tx.send(addr).unwrap();

        let future = listener
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap().0)
            .map(|client| Framed::<_, BytesMut>::new(client))
            .and_then(|client| {
                client.into_future()
                    .map_err(|(err, _)| err)
                    .map(|(msg, _)| msg)
            })
            .and_then(|msg| {
                let msg = msg.unwrap();
                assert_eq!(msg, "hello world");
                Ok(())
            });

        core.run(future).unwrap();
    });

    let mut core = Core::new().unwrap();
    let transport = TcpConfig::new(core.handle())
        .with_upgrade(multiplex::MultiplexConfig);

    let future = transport.dial(rx.recv().unwrap()).unwrap()
        .and_then(|client| client.outbound())
        .map(|server| Framed::<_, BytesMut>::new(server))
        .and_then(|server| server.send("hello world".into()))
        .map(|_| ());

    core.run(future).unwrap();
}
