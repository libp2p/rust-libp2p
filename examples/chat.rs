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

extern crate futures;
extern crate libp2p;
extern crate tokio;
extern crate tokio_stdin;

use futures::prelude::*;
use libp2p::{Transport, core::upgrade, secio, mplex};

fn main() {
    let local_key = secio::SecioKeyPair::ed25519_generated().unwrap();
    let local_peer_id = local_key.to_peer_id();
    println!("Local peer id: {:?}", local_peer_id);

    let transport = libp2p::CommonTransport::new()
        .with_upgrade(secio::SecioConfig::new(local_key))
        .and_then(move |out, endpoint| {
            let peer_id = out.remote_key.into_peer_id();
            let upgrade = upgrade::map(mplex::MplexConfig::new(), move |muxer| (peer_id, muxer));
            upgrade::apply(out.stream, upgrade, endpoint.into())
        });

    let floodsub_topic = libp2p::floodsub::TopicBuilder::new("chat").build();

    let mut swarm = {
        let mut behaviour = libp2p::floodsub::FloodsubBehaviour::new(local_peer_id);
        behaviour.subscribe(floodsub_topic.clone());
        libp2p::Swarm::new(transport, behaviour, libp2p::core::topology::MemoryTopology::empty())
    };

    let addr = libp2p::Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();
    println!("Listening on {:?}", addr);

    if let Some(to_dial) = std::env::args().nth(1) {
        println!("Dialing {:?}", to_dial);
        match to_dial.parse() {
            Ok(to_dial) => { let _ = libp2p::Swarm::dial_addr(&mut swarm, to_dial); },
            Err(err) => println!("Failed to parse address to dial: {:?}", err),
        }
    }

    let mut stdin = tokio_stdin::spawn_stdin_stream_unbounded();

    tokio::run(futures::future::poll_fn(move || -> Result<_, ()> {
        loop {
            match stdin.poll().expect("Error while polling stdin") {
                Async::Ready(Some(key)) => {
                    println!("[stdin.poll()] Ready(Some) – Key: {:?}", key);
                    swarm.publish(&floodsub_topic, vec![key])
                },
                Async::Ready(None) => panic!("Stdin closed"),
                Async::NotReady => {
                    println!("[stdin.poll()] NotReady");
                    break
                }
            };
        }

        loop {
            match swarm.poll().expect("Error while polling swarm") {
                Async::Ready(message) => {
                    println!("Received: {:?}", message);
                },
                Async::NotReady => {
                    println!("[swarm.poll()] NotReady");
                    break
                },
            }
        }

        Ok(Async::NotReady)
    }));
}
