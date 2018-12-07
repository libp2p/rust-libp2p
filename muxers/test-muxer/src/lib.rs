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

use log::{info, trace};
use env_logger;
use tcp::{TcpConfig, TcpTransStream};
use libp2p_core::{
    Transport,
    StreamMuxer,
    muxing::{self, SubstreamRef},
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo}
};
use tokio::{
    codec::length_delimited::Builder,
    runtime::current_thread::Runtime
};
use futures::prelude::*;
use std::{thread, sync::{mpsc, Arc}, fmt::Debug};

pub fn test_muxer<U, O, E>(config: U)
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    U: Debug, // needed for `unwrap()`
    <U as UpgradeInfo>::NamesIter: Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    env_logger::init();
    client_to_server_inbound(config.clone());
    client_to_server_outbound(config.clone());
}

fn client_to_server_inbound<U, O, E>(config: U)
    where
        U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + 'static,
        U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
        U: Debug, // needed for `unwrap()`
        <U as UpgradeInfo>::NamesIter: Send,
        <U as UpgradeInfo>::UpgradeId: Send,
        <U as InboundUpgrade<TcpTransStream>>::Future: Send,
        <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
        E: std::error::Error + Send + Sync + 'static,
        O: StreamMuxer + Send + Sync + 'static ,
        <O as StreamMuxer>::Substream: Send + Sync,
        <O as StreamMuxer>::Substream: Send + Sync,
        <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    // Simulate a client sending a message to a server through a multiplex upgrade.
    let (tx, rx) = mpsc::channel();

    let listener_config = config.clone();
    let bg_thread = thread::spawn(move || {
        let transport =
            TcpConfig::new().with_upgrade(listener_config);

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

    let transport = TcpConfig::new().with_upgrade(config);

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

fn client_to_server_outbound<U, O, E>(config: U)
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    U: Debug, // needed for `unwrap()`
    <U as UpgradeInfo>::NamesIter: Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    let (tx, rx) = mpsc::channel();
    let listener_config = config.clone();
    let thr = thread::spawn(move || {
        let transport = TcpConfig::new().with_upgrade(listener_config);
        let (listener, addr) = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        tx.send(addr).unwrap();

        let mut rt_listener = Runtime::new().unwrap();
        let fut = listener
            // convert stream to future yielding a tuple ("next stream item", "rest of stream")
            .into_future()
            // convert the error type from `(err, ?)` to `err`
            .map_err(|(e, _)| e)
            // stream_item is an `Option<(Future<Item=O, …>, Multiaddr)>` (the ignored tuple item is the rest of the Stream)
            .and_then(|(maybe_muxer, _)| maybe_muxer.unwrap().0)
            .and_then(|muxer: O| {
                // This calls `open_outbound()` on the `StreamMuxer` and returns
                // an `OutboundSubstreamRefWrapFuture<Arc<…>>`.
                // It takes the muxer and builds a future out of it, that, when resolved, yields
                // a substream we can set up the framed codec on.
                muxing::outbound_from_ref_and_wrap(Arc::new(muxer))
            })
            .map(|substream| Builder::new().new_read(substream.unwrap() ))
            .and_then(|substream| {
                substream.into_future()
                    .map_err(|(e, _)| e)
                    .map(|(msg, _)| msg)
            })
            .and_then(|msg| {
                trace!("message received: {:?}", msg);
                Ok(())
            });
        rt_listener.block_on(fut).unwrap();
    });

    let addr = rx.recv().unwrap();
    info!("Listening on {:?}", addr);

    let transport = TcpConfig::new().with_upgrade(config);
    let fut = transport.dial(addr).unwrap()
       .and_then(|muxer: O| {
           muxing::inbound_from_ref_and_wrap(Arc::new(muxer))
       })
       .map(|server: Option<SubstreamRef<Arc<O>>>| Builder::new().new_write(server.unwrap()))
       .and_then(|server| {
           server.send("hello".into())
       });

    let mut rt_dialer = Runtime::new().unwrap();
    let _ = rt_dialer.block_on(fut).unwrap();
    thr.join().unwrap();
}
