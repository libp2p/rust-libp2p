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

use bytes::Bytes;
use env_logger;
use log::{trace, info, warn, error};
use tcp::{TcpConfig, TcpTransStream, TcpListenStream};
use libp2p_core::{
    Multiaddr,
    Transport,
    StreamMuxer,
    transport::upgrade::ListenerStream,
    muxing,
    upgrade::{InboundUpgrade, OutboundUpgrade}, //, UpgradeInfo
};
use tokio::{
    codec::{Framed, LengthDelimitedCodec, length_delimited::Builder},
    runtime::current_thread::Runtime
};
use futures::{
    prelude::*,
    future::Either,
};
use std::{
    fmt::Debug,
    marker::PhantomData,
    sync::{mpsc, Arc, Once, ONCE_INIT},
    thread,
};

use elapsed::{measure_time, ElapsedDuration};

pub struct MuxerTester<U, O, E> {
    _upgrade: PhantomData<U>,
    _muxer: PhantomData<O>,
    _error: PhantomData<E>,
}

static INIT: Once = ONCE_INIT;

impl<U, O, E> MuxerTester<U, O, E>
where
    U: OutboundUpgrade<TcpTransStream, Output = O, Error = E> + Send + Clone + Debug + 'static,
    U: InboundUpgrade<TcpTransStream, Output = O, Error = E>,
    <U as InboundUpgrade<TcpTransStream>>::Future: Send,
    <U as OutboundUpgrade<TcpTransStream>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync + Debug,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    /// Test helpers

    fn init() {
        INIT.call_once(|| env_logger::init())
    }
    /// Given a `Transport` and a `MultiAddr`, returns a framed substream,
    /// either inbound or outbound according to the `inbound` param.
    fn framed_dialler_fut<T>(
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
            .and_then(move |muxer| {
                match inbound {
                    true => Either::A(muxing::inbound_from_ref_and_wrap(Arc::new(muxer))),
                    false => Either::B(muxing::outbound_from_ref_and_wrap(Arc::new(muxer))),
                }
            })
            .map(|substream| Builder::new().new_framed(substream.unwrap()))
    }

    /// Given a `ListenerStream` and an `inbound` boolean, returns a framed substream
    fn framed_listener_fut(
        listener: ListenerStream<TcpListenStream, U>,
        inbound: bool
    ) -> impl Future<
            Item = Framed<muxing::SubstreamRef<Arc<O>>, LengthDelimitedCodec>,
            Error = std::io::Error,
        >
    {
        listener
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(client, _)| client.unwrap().0)
            .and_then(move |muxer| {
                match inbound {
                    true => Either::A(muxing::inbound_from_ref_and_wrap(Arc::new(muxer))),
                    false => Either::B(muxing::outbound_from_ref_and_wrap(Arc::new(muxer))),
                }
            })
            .then(move |substream_result| {
                match substream_result {
                    Err(e) => {
                        error!("[framed_listener_fut] error opening substream: {:?}", e);
                        Err(e)
                    }
                    Ok(Some(subs)) => {
                        info!("[framed_listener_fut] opened substream without error, inbound={:?}", inbound);
                        Ok(Builder::new().new_framed(subs))
                    }
                    Ok(None) => {
                        warn!("[framed_listener_fut] no error, but also no substream, inbound={:?}", inbound);
                        // TODO: I think we're loosing an error here somewhere.
                        // Not sure where yet but it doesn't feel right to let
                        // the Future resolve to a None here
                        Err(std::io::Error::new(std::io::ErrorKind::Other, "something happened but we do not know what :/"))
                    }
                }
            })
    }

    /// Muxer tests

    pub fn empty_payload(config: U) {
        Self::init();
        let (tx, rx) = mpsc::channel();
        let listener_conf = config.clone();
        let thr = thread::spawn(move || {
            let trans = TcpConfig::new().with_upgrade(listener_conf);
            let (listener, addr) = trans
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();
            // Send our address to the connecting side so they know where to find us
            tx.send(addr).unwrap();
            let framed = Self::framed_listener_fut(listener, true);
            let future = framed
                .and_then(|stream| stream.take(2).collect())
                .and_then(|msgs| Ok(assert_eq!(msgs, vec!["", "world"])));
            Runtime::new().unwrap().block_on(future).unwrap();
        });

        let transport = TcpConfig::new().with_upgrade(config);
        let addr = rx.recv().expect("address is valid");
        Runtime::new().unwrap().block_on(
            Self::framed_dialler_fut(transport, addr, false)
                .and_then(|subs| subs.send("".into()))
                .and_then(|subs| subs.send("world".into()))
        ).unwrap();
        thr.join().unwrap();
    }

    pub fn bidirectional(config: U) {
        Self::init();
        let (tx, rx) = mpsc::channel();
        let listener_conf = config.clone();
        let thr = thread::spawn(move || {
            let trans = TcpConfig::new().with_upgrade(listener_conf);
            let (listener, addr) = trans
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();
            // Send our address to the connecting side so they know where to find us
            tx.send(addr).unwrap();
            let framed = Self::framed_listener_fut(listener, true);
            let future = framed
                .and_then(|stream| stream.send("0".into()))
                .and_then(|stream| stream.into_future().map_err(|(e, _)| e))
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
                    stream.send("1".into())
                });

            Runtime::new().unwrap().block_on(future).unwrap();
        });

        let transport = TcpConfig::new().with_upgrade(config);
        let addr = rx.recv().expect("address is valid");
        Runtime::new().unwrap().block_on(
            Self::framed_dialler_fut(transport, addr, false)
                .and_then(|stream| stream.send("a".into()))
                .and_then(|stream| stream.send("b".into()))
                .and_then(|stream| stream.into_future().map_err(|(e, _)| e))
                .and_then(|(message, stream)| {
                    assert_eq!(message.unwrap(), Bytes::from("0"));
                    stream.send("c".into())
                })
                .and_then(|stream| stream.into_future().map_err(|(e, _)| e))
                .and_then(|(message, _)| {
                    assert_eq!(message.unwrap(), Bytes::from("1"));
                    Ok(())
                })
        ).unwrap();
        thr.join().unwrap();
    }

    pub fn client_to_server_inbound(config: U) {
        Self::init();
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

            let future = Self::framed_listener_fut(listener, true)
                .and_then(|stream| stream.take(2).collect())
                .and_then(|msgs| Ok(assert_eq!(msgs, vec!["hello", "world"])));

            Runtime::new().unwrap().block_on(future).unwrap();
        });

        let transport = TcpConfig::new().with_upgrade(config);
        let addr = rx.recv().expect("address is valid");
        Runtime::new().unwrap().block_on(
            Self::framed_dialler_fut(transport, addr, false)
                .and_then(|subs| subs.send("hello".into()))
                .and_then(|subs| subs.send("world".into()))
        ).unwrap();
        bg_thread.join().unwrap();
    }

    pub fn client_to_server_outbound(config: U) {
        Self::init();
        let (tx, rx) = mpsc::channel();
        let listener_config = config.clone();
        let thr = thread::spawn(move || {
            let transport = TcpConfig::new().with_upgrade(listener_config);
            let (listener, addr) = transport
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();
            tx.send(addr).unwrap();

            let framed= Self::framed_listener_fut(listener, false);
            let fut = framed
                .and_then(|stream| stream.take(2).collect())
                .and_then(|msgs| Ok(assert_eq!(msgs, vec!["hello", "world"])));
            Runtime::new().unwrap().block_on(fut).unwrap();
        });

        let addr = rx.recv().unwrap();
        info!("Listening on {:?}", addr);

        let transport = TcpConfig::new().with_upgrade(config);
        Runtime::new().unwrap().block_on(
            Self::framed_dialler_fut(transport, addr, true)
                .and_then(|subs| subs.send("hello".into()))
                .and_then(|subs| subs.send("world".into()))
        ).unwrap();
        thr.join().unwrap();
    }

    pub fn one_megabyte_payload(config: U) {
        Self::init();
        let (tx, rx) = mpsc::channel();
        let listener_conf = config.clone();
        // TODO: 10Mbytes payload
        // TODO: causes buffer overflow for Yamux with default config
        let payload: Vec<u8> = vec![1; 1024 * 1024 * 7];
        let payload_len = payload.len();
        info!("[test] payload size={}", payload_len);

        let thr_builder = thread::Builder::new().name("listener thr".to_string());
        let thr = thr_builder.spawn(move || {
            let trans = TcpConfig::new().with_upgrade(listener_conf);
            let (listener, addr) = trans
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .expect("listen error");
            // Send our address to the connecting side so they know where to find us
            tx.send(addr).unwrap();

            let future = Self::framed_listener_fut(listener, true)
                .and_then(|client| {
                    client
                        .into_future()
                        .map_err(|(err, _)| err)
                        .map(|(msg, _)| msg)
                })
                .inspect(|maybe_msg| trace!("[test, thr] read some={:?}", maybe_msg.is_some()))
                .and_then(|message| {
                    assert!(message.is_some());
                    assert_eq!(message.unwrap().len(), payload_len);
                    Ok(())
                });
            let (elapsed, _) = measure_time(|| {
                Runtime::new().unwrap().block_on(future).unwrap();
            });
            info!("[test, reader] Running the reader future took {}, {}", elapsed, mb_per_sec(payload_len, elapsed));
        }).expect("thread spawn failed");

        let transport = TcpConfig::new().with_upgrade(config);
        let addr = rx.recv().unwrap();

        let (elapsed, _) = measure_time(|| {
            Runtime::new().unwrap().block_on(
                Self::framed_dialler_fut(transport, addr, false)
                    .and_then(|subs| subs.send(payload.into()))
                    .then(|res| {
                        trace!("[test] send result={:?}", res);
                        assert!(res.is_ok());
                        Ok::<_, ()>(())
                    })
            ).expect("sender future works");
        });
        info!("[test, writer] Running the writer future took {}, {}", elapsed, mb_per_sec(payload_len, elapsed));
        thr.join().unwrap();
    }

}
fn mb_per_sec(payload_len: usize, elapsed: ElapsedDuration) -> String {
    let bytes_per_sec = payload_len as f64/(elapsed.duration().as_secs() as f64 + elapsed.duration().subsec_nanos() as f64 * 1e-9);
    let mb_per_sec = bytes_per_sec/(1024.0*1024.0);
    format!("{:.1} Mbyte/sec", mb_per_sec)
}
