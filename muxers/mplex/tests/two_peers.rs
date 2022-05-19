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

use futures::{channel::oneshot, future::poll_fn, prelude::*};
use libp2p_core::{upgrade, StreamMuxer, Transport};
use libp2p_tcp::TcpConfig;

#[test]
fn client_to_server_outbound() {
    // Simulate a client sending a message to a server through a multiplex upgrade.

    let (tx, rx) = oneshot::channel();

    let bg_thread = async_std::task::spawn(async move {
        let mplex = libp2p_mplex::MplexConfig::new();

        let mut transport = TcpConfig::new()
            .and_then(move |c, e| upgrade::apply(c, mplex, e, upgrade::Version::V1));

        let mut listener = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let addr = listener
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        tx.send(addr).unwrap();

        let mut client = listener
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_upgrade()
            .unwrap()
            .0
            .await
            .unwrap();

        client.open_outbound();
        let (mut outbound, _) = poll_fn(|cx| client.poll(cx))
            .await
            .unwrap()
            .into_outbound_substream()
            .unwrap();

        let mut buf = vec![0u8; 11];
        outbound.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello world");
    });

    async_std::task::block_on(async {
        let mplex = libp2p_mplex::MplexConfig::new();
        let mut transport = TcpConfig::new()
            .and_then(move |c, e| upgrade::apply(c, mplex, e, upgrade::Version::V1));

        let mut client = transport.dial(rx.await.unwrap()).unwrap().await.unwrap();
        let mut inbound = loop {
            if let Some(s) = poll_fn(|cx| client.poll(cx))
                .await
                .unwrap()
                .into_inbound_substream()
            {
                break s;
            }
        };
        inbound.write_all(b"hello world").await.unwrap();
        inbound.close().await.unwrap();

        bg_thread.await;
    });
}

#[test]
fn client_to_server_inbound() {
    // Simulate a client sending a message to a server through a multiplex upgrade.

    let (tx, rx) = oneshot::channel();

    let bg_thread = async_std::task::spawn(async move {
        let mplex = libp2p_mplex::MplexConfig::new();

        let mut transport = TcpConfig::new()
            .and_then(move |c, e| upgrade::apply(c, mplex, e, upgrade::Version::V1));

        let mut listener = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let addr = listener
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        tx.send(addr).unwrap();

        let mut client = listener
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_upgrade()
            .unwrap()
            .0
            .await
            .unwrap();

        let mut inbound = loop {
            if let Some(s) = poll_fn(|cx| client.poll(cx))
                .await
                .unwrap()
                .into_inbound_substream()
            {
                break s;
            }
        };

        let mut buf = Vec::new();
        inbound.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello world");
    });

    async_std::task::block_on(async {
        let mplex = libp2p_mplex::MplexConfig::new();
        let mut transport = TcpConfig::new()
            .and_then(move |c, e| upgrade::apply(c, mplex, e, upgrade::Version::V1));

        let mut client = transport.dial(rx.await.unwrap()).unwrap().await.unwrap();

        client.open_outbound();
        let (mut outbound, _) = poll_fn(|cx| client.poll(cx))
            .await
            .unwrap()
            .into_outbound_substream()
            .unwrap();
        outbound.write_all(b"hello world").await.unwrap();
        outbound.close().await.unwrap();

        bg_thread.await;
    });
}

#[test]
fn protocol_not_match() {
    let (tx, rx) = oneshot::channel();

    let _bg_thread = async_std::task::spawn(async move {
        let mplex = libp2p_mplex::MplexConfig::new();

        let mut transport = TcpConfig::new()
            .and_then(move |c, e| upgrade::apply(c, mplex, e, upgrade::Version::V1));

        let mut listener = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let addr = listener
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        tx.send(addr).unwrap();

        let mut client = listener
            .next()
            .await
            .unwrap()
            .unwrap()
            .into_upgrade()
            .unwrap()
            .0
            .await
            .unwrap();

        client.open_outbound();
        let (mut outbound, _) = poll_fn(|cx| client.poll(cx))
            .await
            .unwrap()
            .into_outbound_substream()
            .unwrap();

        let mut buf = Vec::new();
        outbound.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"hello world");
    });

    async_std::task::block_on(async {
        // Make sure they do not connect when protocols do not match
        let mut mplex = libp2p_mplex::MplexConfig::new();
        mplex.set_protocol_name(b"/mplextest/1.0.0");
        let mut transport = TcpConfig::new()
            .and_then(move |c, e| upgrade::apply(c, mplex, e, upgrade::Version::V1));
        match transport.dial(rx.await.unwrap()).unwrap().await {
            Ok(_) => {
                assert!(false, "Dialing should fail here as protocols do not match")
            }
            _ => {}
        }
    });
}
