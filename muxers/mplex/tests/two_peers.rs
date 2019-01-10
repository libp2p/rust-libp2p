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

use libp2p_core::{muxing, Transport};
use libp2p_tcp::TcpConfig;
use futures::prelude::*;
use std::sync::{Arc, mpsc};
use std::thread;
use tokio::{
    codec::length_delimited::Builder,
    runtime::current_thread::Runtime
};

#[test]
fn client_to_server_outbound() {
    // Simulate a client sending a message to a server through a multiplex upgrade.

    let (tx, rx) = mpsc::channel();

    let bg_thread = thread::spawn(move || {
        let transport =
            TcpConfig::new().with_upgrade(libp2p_mplex::MplexConfig::new());

        let (listener, addr) = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        tx.send(addr).unwrap();

        let future = listener
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap().0)
            .and_then(|client| muxing::outbound_from_ref_and_wrap(Arc::new(client)))
            .map(|client| Builder::new().new_read(client.unwrap()))
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

        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
    });

    let transport = TcpConfig::new().with_upgrade(libp2p_mplex::MplexConfig::new());

    let future = transport
        .dial(rx.recv().unwrap())
        .unwrap()
        .and_then(|client| muxing::inbound_from_ref_and_wrap(Arc::new(client)))
        .map(|server| Builder::new().new_write(server.unwrap()))
        .and_then(|server| server.send("hello world".into()))
        .map(|_| ());

    let mut rt = Runtime::new().unwrap();
    let _ = rt.block_on(future).unwrap();
    bg_thread.join().unwrap();
}

#[test]
fn client_to_server_inbound() {
    // Simulate a client sending a message to a server through a multiplex upgrade.

    let (tx, rx) = mpsc::channel();

    let bg_thread = thread::spawn(move || {
        let transport =
            TcpConfig::new().with_upgrade(libp2p_mplex::MplexConfig::new());

        let (listener, addr) = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        tx.send(addr).unwrap();

        let future = listener
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap().0)
            .and_then(|client| muxing::inbound_from_ref_and_wrap(Arc::new(client)))
            .map(|client| Builder::new().new_read(client.unwrap()))
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

        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
    });

    let transport = TcpConfig::new().with_upgrade(libp2p_mplex::MplexConfig::new());

    let future = transport
        .dial(rx.recv().unwrap())
        .unwrap()
        .and_then(|client| muxing::outbound_from_ref_and_wrap(Arc::new(client)))
        .map(|server| Builder::new().new_write(server.unwrap()))
        .and_then(|server| server.send("hello world".into()))
        .map(|_| ());

    let mut rt = Runtime::new().unwrap();
    let _ = rt.block_on(future).unwrap();
    bg_thread.join().unwrap();
}
