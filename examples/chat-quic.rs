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

use env_logger;
use futures::prelude::*;
use libp2p::{self, NetworkBehaviour, core::PublicKey, tokio_codec::{FramedRead, LinesCodec}};
use openssl::{rsa::Rsa, x509::X509};
use quicli::prelude::*;
use structopt::StructOpt;
use std::io;
use void;

// Brief usage instructions:
//
// 1. Generate a private RSA key and self-signed certificate for the listener:
//     $ openssl req -newkey rsa:4096 -nodes -keyout serverkey.pem -x509 -out servercert.pem
//
// 2. Generate a private RSA key and self-signed certificate for the dialer:
//     $ openssl req -newkey rsa:4096 -nodes -keyout clientkey.pem -x509 -out clientcert.pem
//
// 3. Get the PeerId of the listener:
//     $ cargo run --example chat-quic -- peer -c servercert.pem
//
// 4. Start the listener:
//     $ cargo run --example chat-quic -- run -k serverkey.pem -c servercert.pem
//
// 5. Notice the listen port in the output of 4. and start the dialer by using the peer ID from 3.
//     $ cargo run --example chat-quic -- run -k clientkey.pem -c clientcert.pem -d "/ip4/127.0.0.1/udp/<port>/quic/p2p/<peer-id>"

#[derive(Debug, StructOpt)]
enum Cli {
    #[structopt(name = "peer")]
    PeerId(PeerId),
    #[structopt(name = "run")]
    Run(Run)
}

#[derive(Debug, StructOpt)]
struct PeerId {
    #[structopt(long = "cert", short = "c")]
    cert: String
}

#[derive(Debug, StructOpt)]
struct Run {
    #[structopt(long = "key", short = "k")]
    key: String,

    #[structopt(long = "cert", short = "c")]
    cert: String,

    #[structopt(long = "dial", short = "d")]
    dial: Option<String>
}

fn main() -> CliResult {
    env_logger::init();

    match Cli::from_args() {
        Cli::PeerId(args) => peerid(args),
        Cli::Run(args) => run(args)
    }
}

fn peerid(args: PeerId) -> CliResult {
    let certificate = X509::from_pem(read_file(&args.cert)?.as_bytes())?;
    let public_key = PublicKey::Rsa(certificate.public_key()?.public_key_to_der()?);
    println!("Peer ID: {}", public_key.into_peer_id().to_base58());
    Ok(())
}

fn run(args: Run) -> CliResult {
    let private_key = Rsa::private_key_from_pem(read_file(&args.key)?.as_bytes())?;
    let certificate = X509::from_pem(read_file(&args.cert)?.as_bytes())?;
    let public_key = PublicKey::Rsa(certificate.public_key()?.public_key_to_der()?);

    let rt = tokio::runtime::Runtime::new()?;

    let transport = libp2p_quic::QuicConfig::new(rt.executor(), &private_key, &certificate)?;

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
            floodsub: libp2p::floodsub::Floodsub::new(public_key.clone().into_peer_id())
        };
        behaviour.floodsub.subscribe(floodsub_topic.clone());
        libp2p::Swarm::new(transport, behaviour, libp2p::core::topology::MemoryTopology::empty(public_key))
    };

    let addr = libp2p::Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/udp/0/quic".parse()?)?;
    println!("Listening on {:?}", addr);

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
    rt.block_on_all(futures::future::poll_fn(move || {
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

