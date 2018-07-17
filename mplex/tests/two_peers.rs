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
extern crate libp2p_mplex as multiplex;
extern crate libp2p_core as swarm;
extern crate libp2p_tcp_transport as tcp;
extern crate tokio_current_thread;
extern crate tokio_io;

use futures::future::Future;
use futures::{Sink, Stream};
use std::sync::mpsc;
use std::thread;
use swarm::{StreamMuxer, Transport};
use tcp::TcpConfig;
use tokio_io::codec::length_delimited::Framed;

#[test]
fn client_to_server_outbound() {
    // Simulate a client sending a message to a server through a multiplex upgrade.

    let (tx, rx) = mpsc::channel();

    let bg_thread = thread::spawn(move || {
        let transport =
            TcpConfig::new().with_upgrade(multiplex::MplexConfig::new());

        let (listener, addr) = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        tx.send(addr).unwrap();

        let future = listener
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap().map(|v| v.0))
            .and_then(|client| client.outbound())
            .map(|client| Framed::<_, bytes::BytesMut>::new(client.unwrap()))
            .and_then(|client| {
                client
                    .into_future()
                    .map_err(|(err, _)| err)
                    .map(|(msg, _)| msg)
            })
            .and_then(|msg| {
                let msg = msg.unwrap();
                assert_eq!(msg, "hello world");
                Ok(())
            });

        tokio_current_thread::block_on_all(future).unwrap();
    });

    let transport = TcpConfig::new().with_upgrade(multiplex::MplexConfig::new());

    let future = transport
        .dial(rx.recv().unwrap())
        .unwrap()
        .and_then(|client| client.0.inbound())
        .map(|server| Framed::<_, bytes::BytesMut>::new(server.unwrap()))
        .and_then(|server| server.send("hello world".into()))
        .map(|_| ());

    tokio_current_thread::block_on_all(future).unwrap();
    bg_thread.join().unwrap();
}

#[test]
fn client_to_server_inbound() {
    // Simulate a client sending a message to a server through a multiplex upgrade.

    let (tx, rx) = mpsc::channel();

    let bg_thread = thread::spawn(move || {
        let transport =
            TcpConfig::new().with_upgrade(multiplex::MplexConfig::new());

        let (listener, addr) = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        tx.send(addr).unwrap();

        let future = listener
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap().map(|v| v.0))
            .and_then(|client| client.inbound())
            .map(|client| Framed::<_, bytes::BytesMut>::new(client.unwrap()))
            .and_then(|client| {
                client
                    .into_future()
                    .map_err(|(err, _)| err)
                    .map(|(msg, _)| msg)
            })
            .and_then(|msg| {
                let msg = msg.unwrap();
                assert_eq!(msg, "hello world");
                Ok(())
            });

        tokio_current_thread::block_on_all(future).unwrap();
    });

    let transport = TcpConfig::new().with_upgrade(multiplex::MplexConfig::new());

    let future = transport
        .dial(rx.recv().unwrap())
        .unwrap()
        .and_then(|(client, _)| client.outbound())
        .map(|server| Framed::<_, bytes::BytesMut>::new(server.unwrap()))
        .and_then(|server| server.send("hello world".into()))
        .map(|_| ());

    tokio_current_thread::block_on_all(future).unwrap();
    bg_thread.join().unwrap();
}
