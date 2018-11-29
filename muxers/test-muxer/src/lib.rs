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

extern crate libp2p_core;
extern crate tokio;
extern crate tokio_io;
extern crate bytes;
extern crate futures;
#[macro_use]
extern crate log;
extern crate env_logger;
#[macro_use]
extern crate assert_matches;
extern crate libp2p_tcp_transport as tcp;

use tcp::{TcpConfig, TcpTransStream};

use libp2p_core::{
    Transport,
    StreamMuxer,
    muxing,
    transport::Upgrade,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo}
};

use tokio::runtime::{Builder, Runtime};
use tokio_io::codec::length_delimited::Framed;
use futures::{prelude::*, future};
use std::io::Error as IoError;
use std::thread;
use std::sync::mpsc;
use std::fmt::Debug;
use std::sync::Arc;

pub fn test_muxer<U, O, E>(config: U)
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    U: Debug,
    <U as UpgradeInfo>::NamesIter: Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + Debug + 'static,
    O: StreamMuxer + Send + Sync + 'static,
    <O as StreamMuxer>::Substream: Send + Sync,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    env_logger::init();
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
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(client, _)| client.unwrap().0 )
            .and_then(|client| {
                // This returns an OutboundSubstreamRefWrapFuture<Arc<…>>
                muxing::outbound_from_ref_and_wrap(Arc::new(client))
            })
            .map(|client| Framed::<_, bytes::BytesMut>::new(client.unwrap()))
            .and_then(|client| {
                client.into_future()
                    .map_err(|(e, _)| e )
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
       .and_then(|muxer| {
           muxing::inbound_from_ref_and_wrap(Arc::new(muxer))
       })
       .map(|server| Framed::<_, bytes::BytesMut>::new(server.unwrap()))
       .and_then(|server| {
           server.send("hello".into())
       });

    let mut rt_dialer = Runtime::new().unwrap();
    let _ = rt_dialer.block_on(fut).unwrap();
    thr.join().unwrap();
}
