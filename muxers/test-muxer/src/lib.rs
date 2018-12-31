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

extern crate libp2p_tcp as tcp; // TODO: here to satisfy IntelliJ import resolution. Remove before merge.

use bytes::Bytes;
use env_logger;
use log::{trace, debug, info, warn, error};
use tcp::{TcpConfig, TcpTransStream, TcpListenStream};
use libp2p_core::{
    Multiaddr,
    Transport,
    StreamMuxer,
    transport::upgrade::ListenerStream,
    muxing,
    upgrade::{InboundUpgrade, OutboundUpgrade},
    upgrade::UpgradeInfo,
};
use tokio::{
    codec::{Framed, LengthDelimitedCodec, length_delimited::Builder},
    runtime::current_thread::Runtime,
    runtime::Runtime as MtRuntime,
};
use futures::{
    prelude::*,
    future::{Either, loop_fn, Loop}
};
use std::{
    fmt::Debug,
    marker::PhantomData,
    sync::{mpsc, Arc, Once, ONCE_INIT, atomic::{AtomicUsize, Ordering}},
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
    <U as UpgradeInfo>::Info: Send,
    <<U as UpgradeInfo>::InfoIter as std::iter::IntoIterator>::IntoIter: Send,
    E: std::error::Error + Send + Sync + 'static,
    O: StreamMuxer + Send + Sync + 'static ,
    <O as StreamMuxer>::Substream: Send + Sync + Debug,
    <O as StreamMuxer>::OutboundSubstream: Send + Sync,
{
    // Test helpers
    //-------------------

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

    fn localhost() -> Multiaddr {
        const LOCALHOST: &'static str = "/ip4/127.0.0.1/tcp/0";
        LOCALHOST.parse().unwrap()
    }

    // Muxer tests
    //-------------------

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
        // TODO: Yamux: the default config can't cope with a full megabyte payload; removing 10bytes seems to be enough. The reason for this is unclear to me, but is possibly caused by a fast writer sending all the data in a single poll call. I think we should investigate how the read/write interleaving is set up and see if we can't make it work. FWIW this test always pass when running in release mode; not sure why that is.
        // TODO: Mplex: the default split_send_size of 1Kbyte is small which means that the test is very slow. The root cause for this is unclear but my best guess is that we're simply seeing the large syscall overhead present in debug mode. Using a larger chunk size mplex is actually more performant than Yamux so not sure what is the "fix" here, other than using a larger chunk size.
        let payload: Vec<u8> = vec![1; (1024 * 1024) - 10];
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

    // When two connections send data to a single receiver, the data is read in sequence.
    // The first write to finish is the first to be read.
    pub fn two_connections(config: U) {
        Self::init();
        let writer1_conf = config.clone();
        let writer2_conf = config.clone();
        let payload_len = 1024*50;
        info!("[test] Payload size {}", payload_len);

        let transport = TcpConfig::new().with_upgrade(config);
        let (listener, addr) = transport.listen_on(Self::localhost()).expect("can listen on localhost");

        let (tx_wrt1, rx_wrt) = mpsc::channel();
        let tx_wrt2 = tx_wrt1.clone();
        // Writer thread 1
        let addr1 = addr.clone();
        let thr_writer1 = thread::Builder::new().name("writer thread 1".into()).spawn(move || {
            trace!("[test, writer1] Dialling {}", addr1);
            let transport = TcpConfig::new().with_upgrade(writer1_conf);
            let (elapsed, _) = measure_time(|| {
                Runtime::new().unwrap().block_on(
                    Self::framed_dialler_fut(transport, addr1, false)
                        .and_then(|subs| subs.send(vec![111u8; payload_len].into()))
                        .then(|res| {
                            trace!("[test, writer1] send result={:?}", res.is_ok());
                            assert!(res.is_ok());
                            tx_wrt1.send(111u8).unwrap();
                            Ok::<_, ()>(())
                        })
                ).expect("writer1 future works");
            });
            info!("[test, writer1] Running the writer future took {}, {}", elapsed, mb_per_sec(payload_len, elapsed));
        }).expect("spawning writer thread 1 works");

        // Writer thread 2
        let addr2 = addr.clone();
        let thr_writer2 = thread::Builder::new().name("writer thread 2".into()).spawn(move || {
            trace!("[test, writer2] Dialling {}", addr2);
            let transport = TcpConfig::new().with_upgrade(writer2_conf);
            let (elapsed, _) = measure_time(|| {
                Runtime::new().unwrap().block_on(
                    Self::framed_dialler_fut(transport, addr2, false)
                        .and_then(|subs| subs.send(vec![222u8; payload_len].into()))
                        .then(|res| {
                            trace!("[test, writer2] send result={:?}", res.is_ok());
                            assert!(res.is_ok());
                            tx_wrt2.send(222u8).unwrap();
                            Ok::<_, ()>(())
                        })
                ).expect("writer2 future works");
            });
            info!("[test, writer2] Running the writer future took {}, {}", elapsed, mb_per_sec(payload_len, elapsed));
        }).expect("spawning writer thread 2 works");

        // Reader
        let (tx_rd, rx_rd) = mpsc::channel();
        let mut rt = Runtime::new().unwrap();
        let fut = listener
            .take(2)
            .for_each(|(muxer, _addr)| {
                muxer
                    .and_then(|muxer| muxing::inbound_from_ref_and_wrap(Arc::new(muxer)))
                    .and_then(|substream| Ok(Builder::new().new_framed(substream.unwrap())))
                    .and_then(|framed_stream| framed_stream.take(1).collect())
                    .and_then(|msgs| {
                        debug!("[test, reader] read {} bytes", msgs[0].len());
                        tx_rd.send(msgs[0][0]).unwrap();
                        assert_eq!(msgs[0].len(), payload_len);
                        Ok(())
                    })
            });
        let (elapsed, _) = measure_time(|| {
            rt.block_on(fut)
        });
        info!("[test, reader] Running the reader future took {}, {}", elapsed, mb_per_sec(payload_len, elapsed));

        let first_bytes_read = rx_rd.iter().take(2).collect::<Vec<u8>>();
        let first_bytes_written= rx_wrt.iter().take(2).collect::<Vec<u8>>();
        // Data is read in the same order it is written; there is no interleaving of writes
        assert_eq!(first_bytes_written, first_bytes_read);

        thr_writer1.join().expect("joining writer thread 1 works");
        thr_writer2.join().expect("joining writer thread 2 works");
    }

    pub fn one_hundred_small_streams(config: U) {
        Self::init();
        const N_STREAMS : usize = 100;
        static RX_COUNT : AtomicUsize = AtomicUsize::new(0);
        static TX_OK_COUNT : AtomicUsize = AtomicUsize::new(0);
        static TX_ERR_COUNT : AtomicUsize = AtomicUsize::new(0);
        let (tx, rx) = mpsc::channel();
        let listener_conf = config.clone();

        let thr_builder = thread::Builder::new().name("listener thr".to_string());
        let thr = thr_builder.spawn(move || {
            let trans = TcpConfig::new().with_upgrade(listener_conf);
            let (listener, addr) = trans
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).expect("listen error");

            // Send our address to the connecting side so they know where to find us
            tx.send(addr).unwrap();
            let rt = MtRuntime::new().unwrap();
            let future = listener
                .take(1) // Single connection
                .for_each(|(listener_upgrade_fut, _)| {
                    listener_upgrade_fut.and_then(|muxer: O| {
                        trace!("Reader – incoming connection");
                        let mx = Arc::new(muxer);
                        // Loop-future to set up reading from each incoming stream
                        let read_loop = loop_fn((0, mx), |(count, mx)| {
                            trace!("--> Reader Loop {} – polling inbound", count);
                            match mx.poll_inbound() {
                                Ok(Async::Ready(Some(stream))) => {
                                    // Loop-future to read all incoming data from the stream
                                    let read_fut = loop_fn((0, stream, mx.clone()), |(read_count, mut stream, mx)| {
                                        trace!("--––> Reader fut {} – read_substream", read_count);
                                        let mut buf = vec![0;10];
                                        match mx.read_substream(&mut stream, &mut buf) {
                                            Ok(Async::Ready(num_read)) => {
                                                if num_read == 0 {
                                                    Ok(Loop::Break(()))
                                                } else {
                                                    debug!("Reader fut {} – Read {} bytes from stream", read_count, num_read);
                                                    RX_COUNT.fetch_add(1, Ordering::SeqCst);
                                                    Ok(Loop::Continue((read_count + 1, stream, mx)))
                                                }
                                            }
                                            Ok(Async::NotReady) => {
                                                trace!("Reader fut {} – NotReady", read_count);
                                                Ok(Loop::Continue((read_count + 1, stream, mx)))
                                            }
                                            Err(e) => {
                                                error!("Reader fut {} – error={:?}", read_count, e);
                                                Ok(Loop::Break(()))
                                            }
                                        }
                                    });
                                    tokio::spawn(read_fut);
                                    Ok(Loop::Continue((count + 1, mx)))
                                }
                                Ok(Async::NotReady) => { Ok(Loop::Continue((count + 1, mx))) }
                                Ok(Async::Ready(None)) => {
                                    debug!("Reader Loop {} – Async::Ready(None)", count);
                                    Ok(Loop::Break(()))
                                }
                                Err(e) => {
                                    warn!("Reader Loop {} – error={:?}", count, e);
                                    Ok(Loop::Break(()))
                                    // TODO: does it help to loop here?
                                    // Sometimes it seems like it does. :/
                                    // Ok(Loop::Continue((count + 1, mx)))

                                }
                            }
                        });
                        tokio::spawn(read_loop);
                        trace!("Reader – incoming connection – DONE");
                        Ok(())
                    })
                });
            rt.block_on_all(future).unwrap();
        }).expect("thread spawn failed");

        let transport = TcpConfig::new().with_upgrade(config);
        let addr = rx.recv().unwrap();

        let sender_fut = transport.dial(addr).unwrap()
            .and_then(|muxer| {
                trace!("Sender – START");
                let muxer = Arc::new(muxer);
                let send_loop = loop_fn((0, muxer), |(count, muxer)| {
                    trace!("<–– Sender Loop {} – START", count);
                    if count >= N_STREAMS {
                        debug!("<–– Sender Loop {} – done sending.", count);
                        Ok(Loop::Break(()))
                    } else {
                        let c = count;
                        let send = muxing::outbound_from_ref_and_wrap(muxer.clone())
                            .map(|s| Builder::new().new_framed(s.unwrap()))
                            .and_then(|framed| framed.send("abc".into()))
                            .then(move |result| {
                                if result.is_ok() {
                                    trace!("Sender Loop {} – send ok, result={:?}", c, result);
                                    TX_OK_COUNT.fetch_add(1, Ordering::SeqCst);
                                } else {
                                    warn!("Sender Loop {} – send was not ok. Result={:?}", c, result);
                                    TX_ERR_COUNT.fetch_add(1, Ordering::SeqCst);
                                }
                                // TODO: Pausing here seems to help with rare
                                // random read misses, probably because the
                                // sender closes the connection before the
                                // reader has had time to read all the data.
                                // Find out why we miss some data every now and
                                // then. This happens both with mplex and yamux.
                                // For yamux the error seen on the listening
                                // side is of different kinds and does not seem
                                // to be relevant: "Broken Pipe", "Connection
                                // reset by peer" and "Protocol wrong type for
                                // socket". To repeat remove the sleep.
                                thread::sleep(std::time::Duration::from_millis(10));
                                Ok::<_, ()>(())
                            });
                        tokio::spawn(send);
                        Ok(Loop::Continue((count + 1, muxer)))
                    }
                });
                tokio::spawn(send_loop);

                trace!("Sender – DONE");
                Ok(())
            });

        let rt = MtRuntime::new().unwrap();
        rt.block_on_all(sender_fut).unwrap();
        thr.join().unwrap();
        info!("[test, reader] read data {:?} times", RX_COUNT.load(Ordering::SeqCst));
        info!("[test, writer] sent data {} times successfully and {} times there was an error", TX_OK_COUNT.load(Ordering::SeqCst), TX_ERR_COUNT.load(Ordering::SeqCst));
        assert_eq!(TX_OK_COUNT.load(Ordering::SeqCst), N_STREAMS);
        assert_eq!(RX_COUNT.load(Ordering::SeqCst), N_STREAMS);
    }
}
fn mb_per_sec(payload_len: usize, elapsed: ElapsedDuration) -> String {
    let bytes_per_sec = payload_len as f64/(elapsed.duration().as_secs() as f64 + elapsed.duration().subsec_nanos() as f64 * 1e-9);
    let mb_per_sec = bytes_per_sec/(1024.0*1024.0);
    format!("{:.1} Mbyte/sec", mb_per_sec)
}
