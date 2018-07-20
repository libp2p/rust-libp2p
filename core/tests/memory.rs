extern crate bytes;
extern crate futures;
extern crate libp2p_core;
extern crate tokio_codec;
extern crate tokio_current_thread;

use bytes::Bytes;
use futures::{future::{self, Either, Loop}, prelude::*, sync::mpsc};
use std::{io, iter};
use libp2p_core::{transport::memory, swarm, ConnectionUpgrade, Endpoint, Transport};
use tokio_codec::{BytesCodec, Framed};

#[test]
fn echo() {

    #[derive(Clone)]
    struct Echo(mpsc::UnboundedSender<()>);

    impl<Maf: 'static> ConnectionUpgrade<memory::Channel<Bytes>, Maf> for Echo {
        type NamesIter = iter::Once<(Bytes, ())>;
        type UpgradeIdentifier = ();
        type Output = ();
        type MultiaddrFuture = Maf;
        type Future = Box<Future<Item=(Self::Output, Self::MultiaddrFuture), Error=io::Error>>;

        fn protocol_names(&self) -> Self::NamesIter {
            iter::once(("/echo/1.0.0".into(), ()))
        }

        fn upgrade(self, chan: memory::Channel<Bytes>, _: (), e: Endpoint, maf: Maf) -> Self::Future {
            let chan = Framed::new(chan, BytesCodec::new());
            match e {
                Endpoint::Listener => {
                    let future = future::loop_fn(chan, move |chan| {
                        chan.into_future()
                            .map_err(|(e, _)| e)
                            .and_then(move |(msg, chan)| {
                                if let Some(msg) = msg {
                                    println!("listener received: {:?}", msg);
                                    Either::A(chan.send(msg.freeze()).map(Loop::Continue))
                                } else {
                                    println!("listener received EOF at destination");
                                    Either::B(future::ok(Loop::Break(())))
                                }
                            })
                    });
                    Box::new(future.map(move |()| ((), maf))) as Box<_>
                }
                Endpoint::Dialer => {
                    let future = chan.send("hello world".into())
                        .and_then(|chan| {
                            chan.into_future().map_err(|(e, _)| e).map(|(n,_ )| n)
                        })
                        .and_then(|msg| {
                            println!("dialer received: {:?}", msg.unwrap());
                            self.0.send(())
                                .map(|_| ())
                                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                        });
                    Box::new(future.map(move |()| ((), maf))) as Box<_>
                }
            }
        }
    }

    let (finish_tx, finish_rx) = mpsc::unbounded();
    let echo = Echo(finish_tx);

    let (dialer, listener) = memory::connector();

    let dialer = dialer.with_dummy_muxing().with_upgrade(echo.clone());
    let listener = listener.with_dummy_muxing().with_upgrade(echo);

    let (control, future) = swarm(listener, |sock, _addr| Ok(sock));

    control.listen_on("/memory".parse().expect("/memory is a valid multiaddr")).unwrap();
    control.dial("/memory".parse().expect("/memory is a valid multiaddr"), dialer).unwrap();

    let finish_rx = finish_rx.into_future()
        .map(|_| ())
        .map_err(|((), _)| io::Error::new(io::ErrorKind::Other, "receive error"));

    let future = future.select(finish_rx)
        .map(|_| ())
        .map_err(|(e, _)| e);

    tokio_current_thread::block_on_all(future).unwrap();
}

