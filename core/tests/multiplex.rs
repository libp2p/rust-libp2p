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
extern crate libp2p_core;
extern crate libp2p_tcp_transport;
extern crate tokio_current_thread;
extern crate tokio_io;
extern crate env_logger;


use bytes::BytesMut;
use futures::future::Future;
use futures::{Sink, Stream};
use libp2p_core::{muxing, Multiaddr, MuxedTransport, Transport, transport};
use std::sync::{atomic, Arc};
use std::thread;
use tokio_io::codec::length_delimited::Framed;

// Ensures that a transport is only ever used once for dialing.
#[derive(Debug)]
struct OnlyOnce<T>(T, atomic::AtomicBool);
impl<T> From<T> for OnlyOnce<T> {
    fn from(tp: T) -> OnlyOnce<T> {
        OnlyOnce(tp, atomic::AtomicBool::new(false))
    }
}
impl<T: Clone> Clone for OnlyOnce<T> {
    fn clone(&self) -> Self {
        OnlyOnce(
            self.0.clone(),
            atomic::AtomicBool::new(self.1.load(atomic::Ordering::SeqCst)),
        )
    }
}
impl<T: Transport + 'static> Transport for OnlyOnce<T> {
    type Output = T::Output;
    type MultiaddrFuture = T::MultiaddrFuture;
    type Listener = T::Listener;
    type ListenerUpgrade = T::ListenerUpgrade;
    type Dial = T::Dial;
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        Ok(self.0.listen_on(addr).unwrap_or_else(|_| panic!()))
    }
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        assert!(!self.1.swap(true, atomic::Ordering::SeqCst));
        Ok(self.0.dial(addr).unwrap_or_else(|_| panic!()))
    }
    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.0.nat_traversal(a, b)
    }
}



#[test]
fn client_to_server_outbound() {
    // A client opens a connection to a server, then an outgoing substream, then sends a message
    // on that substream.

    let _ = env_logger::try_init();
    let (tx, rx) = transport::connector();

    let bg_thread = thread::spawn(move || {
        let future = rx
            .with_upgrade(multiplex::MplexConfig::new())
            .into_connection_reuse()
            .listen_on("/memory".parse().unwrap())
            .unwrap_or_else(|_| panic!()).0
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| { client.expect("Client could not be created") })
            .map(|client| client.0)
            .map(|client| Framed::<_, BytesMut>::new(client))
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

    let future = tx
        .with_upgrade(multiplex::MplexConfig::new())
        .dial("/memory".parse().unwrap())
        .unwrap_or_else(|_| panic!())
        .and_then(|client| muxing::outbound_from_ref_and_wrap(Arc::new(client.0)))
        .map(|server| Framed::<_, BytesMut>::new(server.unwrap()))
        .and_then(|server| server.send("hello world".into()))
        .map(|_| ());

    tokio_current_thread::block_on_all(future).unwrap();
    bg_thread.join().unwrap();
}

#[test]
fn connection_reused_for_dialing() {
    // A client dials the same multiaddress twice in a row. We check that it uses two substreams
    // instead of opening two different connections.

    let _ = env_logger::try_init();
    let (tx, rx) = transport::connector();

    let bg_thread = thread::spawn(move || {
        let future = OnlyOnce::from(rx)
            .with_upgrade(multiplex::MplexConfig::new())
            .into_connection_reuse()
            .listen_on("/memory".parse().unwrap())
            .unwrap_or_else(|_| panic!()).0
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, rest)| client.unwrap().map(move |c| (c.0, rest)))
            .map(|(client, rest)| (Framed::<_, BytesMut>::new(client), rest))
            .and_then(|(client, rest)| {
                client
                    .into_future()
                    .map(|v| (v, rest))
                    .map_err(|(err, _)| err)
            })
            .and_then(|((msg, _), rest)| {
                let msg = msg.unwrap();
                assert_eq!(msg, "hello world");
                Ok(rest)
            })
            .flatten_stream()
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap())
            .map(|client| client.0)
            .map(|client| Framed::<_, BytesMut>::new(client))
            .and_then(|client| client.into_future().map_err(|(err, _)| err))
            .and_then(|(msg, _)| {
                let msg = msg.unwrap();
                assert_eq!(msg, "second message");
                Ok(())
            });

        tokio_current_thread::block_on_all(future).unwrap();
    });

    let transport = OnlyOnce::from(tx)
        .with_upgrade(multiplex::MplexConfig::new())
        .into_connection_reuse();

    let future = transport
        .clone()
        .dial("/memory".parse().unwrap())
        .unwrap_or_else(|_| panic!())
        .map(|server| Framed::<_, BytesMut>::new(server.0))
        .and_then(|server| server.send("hello world".into()))
        .and_then(|first_connec| {
            transport
                .clone()
                .dial("/memory".parse().unwrap())
                .unwrap_or_else(|_| panic!())
                .map(|server| Framed::<_, BytesMut>::new(server.0))
                .map(|server| (first_connec, server))
        })
        .and_then(|(_first, second)| second.send("second message".into()))
        .map(|_| ());

    tokio_current_thread::block_on_all(future).unwrap();
    bg_thread.join().unwrap();
}

#[test]
fn use_opened_listen_to_dial() {
    // A server waits for an incoming substream and a message on it, then opens an outgoing
    // substream on that same connection, that the client has to accept. The client then sends a
    // message on that new substream.

    let _ = env_logger::try_init();
    let (tx, rx) = transport::connector();

    let bg_thread = thread::spawn(move || {
        let future = OnlyOnce::from(rx)
            .with_upgrade(multiplex::MplexConfig::new())
            .listen_on("/memory".parse().unwrap())
            .unwrap_or_else(|_| panic!()).0
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap())
            .map(|client| Arc::new(client.0))
            .and_then(|c| {
                let c2 = c.clone();
                muxing::inbound_from_ref_and_wrap(c.clone()).map(move |i| (c2, i))
            })
            .map(|(muxer, client)| (muxer, Framed::<_, BytesMut>::new(client.unwrap())))
            .and_then(|(muxer, client)| {
                client
                    .into_future()
                    .map(move |msg| (muxer, msg))
                    .map_err(|(err, _)| err)
            })
            .and_then(|(muxer, (msg, _))| {
                let msg = msg.unwrap();
                assert_eq!(msg, "hello world");
                muxing::outbound_from_ref_and_wrap(muxer)
            })
            .map(|client| Framed::<_, BytesMut>::new(client.unwrap()))
            .and_then(|client| client.into_future().map_err(|(err, _)| err))
            .and_then(|(msg, _)| {
                let msg = msg.unwrap();
                assert_eq!(msg, "second message");
                Ok(())
            });

        tokio_current_thread::block_on_all(future).unwrap();
    });

    let transport = OnlyOnce::from(tx)
        .with_upgrade(multiplex::MplexConfig::new())
        .into_connection_reuse();

    let future = transport
        .clone()
        .dial("/memory".parse().unwrap())
        .unwrap_or_else(|_| panic!())
        .map(|server| Framed::<_, BytesMut>::new(server.0))
        .and_then(|server| server.send("hello world".into()))
        .and_then(|first_connec| {
            transport
                .clone()
                .next_incoming()
                .and_then(|server| server)
                .map(|server| Framed::<_, BytesMut>::new(server.0))
                .map(|server| (first_connec, server))
        })
        .and_then(|(_first, second)| second.send("second message".into()))
        .map(|_| ());

    tokio_current_thread::block_on_all(future).unwrap();
    bg_thread.join().unwrap();
}
