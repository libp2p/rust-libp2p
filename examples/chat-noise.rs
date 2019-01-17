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

extern crate env_logger;
extern crate futures;
extern crate libp2p;
extern crate quicli;
extern crate structopt;
extern crate tokio;

use futures::prelude::*;
use libp2p::{
    NetworkBehaviour, Transport, InboundUpgradeExt, OutboundUpgradeExt,
    core::{PublicKey, PeerId},
    tokio_codec::{FramedRead, LinesCodec}
};
use quicli::prelude::*;
use structopt::StructOpt;
use std::io;
use tokio::runtime::Runtime;

#[derive(Debug, StructOpt)]
struct Cli {
    #[structopt(long = "dial", short = "d")]
    dial: Option<String>
}

fn main() -> CliResult {
    env_logger::init();

    let args = Cli::from_args();

    let (transport, pubkey) = {
        let keypair = libp2p::noise::Keypair::fresh();
        let pubkey = keypair.public().clone();
        let tcp = libp2p::tcp::TcpConfig::new();
        let transport = libp2p::noise::NoiseConfig::new(tcp, keypair)
            .and_then(|(remote_peer_id1, conn), endpoint| {
                let remote_peer_id2 = remote_peer_id1.clone();
                let yamux = libp2p::yamux::Config::default();
                let mplex = libp2p::mplex::MplexConfig::new();
                let upgrade = libp2p::core::upgrade::SelectUpgrade::new(yamux, mplex)
                    .map_inbound(move |muxer| (remote_peer_id1, muxer))
                    .map_outbound(move |muxer| (remote_peer_id2, muxer));
                libp2p::core::upgrade::apply(conn, upgrade, endpoint)
                    .map(|(id, muxer)| (id, libp2p::core::muxing::StreamMuxerBox::new(muxer)))
            });
        (transport, pubkey)
    };

    let floodsub_topic = libp2p::floodsub::TopicBuilder::new("chat").build();

    #[derive(NetworkBehaviour)]
    struct MyBehaviour<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite> {
        floodsub: libp2p::floodsub::Floodsub<TSubstream>
    }

    impl<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite> libp2p::core::swarm::NetworkBehaviourEventProcess<libp2p::floodsub::FloodsubEvent> for MyBehaviour<TSubstream> {
        // Called when `floodsub` produces an event.
        fn inject_event(&mut self, message: libp2p::floodsub::FloodsubEvent) {
            if let libp2p::floodsub::FloodsubEvent::Message(message) = message {
                println!("Received: '{:?}' from {:?}", String::from_utf8_lossy(&message.data), message.source);
            }
        }
    }

    // Create a Swarm to manage peers and events
    let mut swarm = {
        let mut behaviour = MyBehaviour {
            floodsub: libp2p::floodsub::Floodsub::new(PeerId::encode(pubkey.as_ref()))
        };
        behaviour.floodsub.subscribe(floodsub_topic.clone());
        let fake = PublicKey::Ed25519(pubkey.as_ref().into()); // FIXME
        libp2p::Swarm::new(transport, behaviour, libp2p::core::topology::MemoryTopology::empty(fake))
    };

    let addr = libp2p::Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse()?)?;
    println!("Listening on {:?}; curve25519 key = {}", addr, pubkey.base58_encoded());

    // Reach out to another node if specified
    if let Some(to_dial) = args.dial {
        match to_dial.parse() {
            Ok(addr) => {
                match libp2p::Swarm::dial_addr(&mut swarm, addr) {
                    Ok(_) => println!("Dialed {:?}", to_dial),
                    Err(e) => println!("Dial {:?} failed: {:?}", to_dial, e)
                }
            }
            Err(err) => println!("Failed to parse address to dial: {:?}", err)
        }
    }

    // Read full lines from stdin
    let stdin = tokio_stdin_stdout::stdin(0);
    let mut framed_stdin = FramedRead::new(stdin, LinesCodec::new());

    // Kick it off
    Runtime::new()?.block_on_all(futures::future::poll_fn(move || {
        loop {
            match framed_stdin.poll()? {
                Async::Ready(Some(line)) => swarm.floodsub.publish(&floodsub_topic, line.as_bytes()),
                Async::Ready(None) => return Err(io::Error::new(io::ErrorKind::Other, "stdin closed")),
                Async::NotReady => break
            };
        }

        loop {
            match swarm.poll()? {
                Async::Ready(Some(_)) => {}
                Async::Ready(None) | Async::NotReady => break
            }
        }

        Ok(Async::NotReady)
    }))?;

    Ok(())
}

