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
extern crate rand;
extern crate tokio_current_thread;
extern crate tokio_io;

use futures::{future, future::Future};
use libp2p_core::Transport;
use libp2p_tcp_transport::TcpConfig;
use std::sync::{atomic, Arc};

#[test]
fn lots_of_swarms() {
    let transport = TcpConfig::new().with_dummy_muxing();

    let mut swarm_controllers = Vec::new();
    let mut swarm_futures = Vec::new();

    let num_established = Arc::new(atomic::AtomicUsize::new(0));

    for _ in 0 .. 200 + rand::random::<usize>() % 100 {
        let esta = num_established.clone();
        let (ctrl, fut) = libp2p_core::swarm(
            transport.clone(),
            move |socket, _| {
                esta.fetch_add(1, atomic::Ordering::SeqCst);
                future::ok(socket).join(future::empty::<(), _>()).map(|_| ())
            }
        );

        swarm_controllers.push(ctrl);
        swarm_futures.push(fut);
    }

    let mut addresses = Vec::new();
    for ctrl in &swarm_controllers {
        addresses.push(ctrl.listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap());
    }

    let mut dial_fut = Vec::new();
    let num_to_establish = 150 + rand::random::<usize>() % 150;
    for _ in 0 .. num_to_establish {
        let ctrl = swarm_controllers.get(rand::random::<usize>() % swarm_controllers.len()).unwrap();
        let to_dial = addresses.get(rand::random::<usize>() % addresses.len()).unwrap();
        dial_fut.push(ctrl.dial(to_dial.clone(), transport.clone()).unwrap());
    }

    let select_swarm = future::select_all(swarm_futures).map(|_| ()).map_err(|(err, _, _)| err);
    let select_dial = future::join_all(dial_fut).map(|_| ());
    let combined = select_swarm.select(select_dial).map(|_| ()).map_err(|(err, _)| err);
    tokio_current_thread::block_on_all(combined).unwrap();

    assert_eq!(num_established.load(atomic::Ordering::SeqCst), num_to_establish * 2);
}
