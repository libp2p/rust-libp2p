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
extern crate libp2p_tcp_transport as tcp;

use log::{trace, debug, info};
use env_logger;
use tcp::{TcpConfig, TcpTransStream, TcpListenStream};
use libp2p_core::{
    Transport,
    StreamMuxer,
    transport::upgrade::ListenerStream,
    muxing,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
    Multiaddr,
};
use tokio::{
    codec::{Framed, LengthDelimitedCodec, length_delimited::Builder},
    runtime::current_thread::Runtime
};
use bytes::Bytes;
use futures::prelude::*;
use futures::future::Either;
use std::{thread, sync::{mpsc, Arc}, fmt::Debug};

mod helpers {
    use super::*;

    pub(crate) fn framed_dialler_fut<T>(
        transport: T,
        addr: Multiaddr,
        inbound: bool,
    ) -> impl Future<
        Item = Framed<muxing::SubstreamRef<Arc<T::Output>>, LengthDelimitedCodec>,
        Error = std::io::Error,
    >
        where
            T: Transport + Debug,
            T::Output: StreamMuxer + Send + Sync + 'static,
    {
        transport
            .dial(addr)
            .unwrap()
            .and_then(move |server| {
                match inbound {
                    true => Either::A(muxing::inbound_from_ref_and_wrap(Arc::new(server))),
                    false => Either::B(muxing::outbound_from_ref_and_wrap(Arc::new(server))),
                }
            })
            .map(|server| Builder::new().new_framed(server.unwrap()))
    }

    pub(crate) fn framed_listener_fut<U, O, E>(
        listener: ListenerStream<TcpListenStream, U>,
        inbound: bool
    )
        -> impl Future<
            Item = Framed<muxing::SubstreamRef<Arc<O>>, LengthDelimitedCodec>,
            Error = std::io::Error,
        >
    where
        U: InboundUpgrade<TcpTransStream, Output = O, Error = E> + Debug + Send + Clone + 'static,
        <U as UpgradeInfo>::NamesIter: Send,
        <U as UpgradeInfo>::UpgradeId: Send,
        <U as InboundUpgrade<TcpTransStream>>::Future: Send,
        E: std::error::Error + Send + Sync + 'static,
        O: StreamMuxer + Send + Sync + 'static,
        <O as StreamMuxer>::Substream: Send + Sync,
    {
        listener
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(client, _)| client.unwrap().0)
            .and_then(move |client| {
                match inbound {
                    true => Either::A(muxing::inbound_from_ref_and_wrap(Arc::new(client))),
                    false => Either::B(muxing::outbound_from_ref_and_wrap(Arc::new(client))),
                }
            })
            .map(|client| Builder::new().new_framed(client.unwrap()))
    }

}

pub fn test_muxer<U, O, E>(config: U)
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + Debug + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    <U as UpgradeInfo>::NamesIter: Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    env_logger::init();
    info!("\nCalling inbound\n");
    client_to_server_inbound(config.clone());
    info!("\nCalling outbound\n");
    client_to_server_outbound(config.clone());

    info!("\nSending empty payload works fine\n");
    send_empty_payload(config.clone());

    info!("\nBidirectional\n");
    bidirectional(config.clone());
/*
Then I would try all the things that the implementor didn't necessarily think of: send packets of 0 bytes, large packets (eg. 20MB), try cancels an outbound substream after it has succeeded
- zero-sized payloads
- bidirectional messages
- large messages
- unexpected cancel
Etc.
*/
}

fn bidirectional<U, O, E>(config: U)
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + Debug + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    <U as UpgradeInfo>::NamesIter: Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    let (tx, rx) = mpsc::channel();
    let listener_conf = config.clone();
    let thr = thread::spawn(move || {
        let trans = TcpConfig::new().with_upgrade(listener_conf);
        let (listener, addr) = trans
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        // Send our address to the connecting side so they know where to find us
        tx.send(addr).unwrap();
        let framed = helpers::framed_listener_fut(listener, true);
        let future = framed
            .and_then(|stream| stream.send("0".into()))
            .and_then(|stream| {
                stream.into_future().map_err(|(e, _)| e)
            })
            .and_then(|(msg, stream)| {
                assert_eq!(msg.unwrap(), Bytes::from("a"));
                stream.into_future().map_err(|(e, _)| e)
            })
            .and_then(|(msg, stream)| {
                assert_eq!(msg.unwrap(), Bytes::from("b"));
                stream.into_future().map_err(|(e, _)| e)
            })
            .and_then(|(msg, stream)| {
                assert_eq!(msg.unwrap(), Bytes::from("c"));
                Ok(())
            });

        Runtime::new().unwrap().block_on(future).unwrap();
    });

    let transport = TcpConfig::new().with_upgrade(config);
    let addr = rx.recv().expect("address is valid");
    Runtime::new().unwrap().block_on(
        helpers::framed_dialler_fut(transport, addr, false)
            .and_then(|stream| stream.send("a".into()))
            .and_then(|stream| stream.send("b".into()))
            .and_then(|stream| {
                stream.into_future().map_err(|(e, _)| e)
            })
            .and_then(|(message, stream)| {
                assert_eq!(message.unwrap(), Bytes::from("0"));
                stream.send("c".into())
            })
            .and_then(|_| Ok(()))
    ).unwrap();
    thr.join().unwrap();
}

fn send_empty_payload<U, O, E>(config: U)
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + Debug + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    <U as UpgradeInfo>::NamesIter: Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    let (tx, rx) = mpsc::channel();
    let listener_conf = config.clone();
    let thr = thread::spawn(move || {
        let trans = TcpConfig::new().with_upgrade(listener_conf);
        let (listener, addr) = trans
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        // Send our address to the connecting side so they know where to find us
        tx.send(addr).unwrap();
        let framed = helpers::framed_listener_fut(listener, true);
        let future = framed
            .and_then(|stream| stream.take(2).collect())
            .and_then(|msgs| Ok(assert_eq!(msgs, vec!["", "world"])));
        Runtime::new().unwrap().block_on(future).unwrap();
    });

    let transport = TcpConfig::new().with_upgrade(config);
    let addr = rx.recv().expect("address is valid");
    Runtime::new().unwrap().block_on(
        helpers::framed_dialler_fut(transport, addr, false)
            .and_then(|subs| subs.send("".into()))
            .and_then(|subs| subs.send("world".into()))
    ).unwrap();
    thr.join().unwrap();
}

fn client_to_server_inbound<U, O, E>(config: U)
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + Debug + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    <U as UpgradeInfo>::NamesIter: Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    // Simulate a client sending a message to a server.
    let (tx, rx) = mpsc::channel();

    let listener_config = config.clone();
    let bg_thread = thread::spawn(move || {
        let transport = TcpConfig::new().with_upgrade(listener_config);

        let (listener, addr) = transport
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        // Send our address to the connecting side so they know where to find us
        tx.send(addr).unwrap();

        let framed = helpers::framed_listener_fut(listener, true);
        let future = framed
            .and_then(|stream| stream.take(2).collect())
            .and_then(|msgs| Ok(assert_eq!(msgs, vec!["hello", "world"])));

        Runtime::new().unwrap().block_on(future).unwrap();
    });

    let transport = TcpConfig::new().with_upgrade(config);
    let addr = rx.recv().expect("address is valid");
    Runtime::new().unwrap().block_on(
        helpers::framed_dialler_fut(transport, addr, false)
            .and_then(|subs| subs.send("hello".into()))
            .and_then(|subs| subs.send("world".into()))
    ).unwrap();
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

        let framed= helpers::framed_listener_fut(listener, false);
        let fut = framed
            .and_then(|stream| stream.take(2).collect())
            .and_then(|msgs| Ok(assert_eq!(msgs, vec!["hello", "world"])));
        Runtime::new().unwrap().block_on(fut).unwrap();
    });

    let addr = rx.recv().unwrap();
    info!("Listening on {:?}", addr);

    let transport = TcpConfig::new().with_upgrade(config);
    Runtime::new().unwrap().block_on(
        helpers::framed_dialler_fut(transport, addr, true)
            .and_then(|subs| subs.send("hello".into()))
            .and_then(|subs| subs.send("world".into()))
    ).unwrap();
    thr.join().unwrap();
}
