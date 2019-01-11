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
extern crate openssl;
extern crate quicli;
extern crate structopt;
extern crate tokio;
extern crate void;

use futures::prelude::*;
use libp2p::{NetworkBehaviour, core::PublicKey, tokio_codec::{FramedRead, LinesCodec}};
use openssl::{rsa::Rsa, x509::X509};
use quicli::prelude::*;
use structopt::StructOpt;
use std::io;


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
// 5. Notice the listen port in the output of 4. and start the dialer:
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

    // Set up a an encrypted DNS-enabled TCP Transport over the Mplex and Yamux protocols
    let transport = libp2p_quic::QuicConfig::new(rt.executor(), &private_key, &certificate)?;

    // Create a Floodsub topic
    let floodsub_topic = libp2p::floodsub::TopicBuilder::new("chat").build();

    // We create a custom network behaviour that combines floodsub and mDNS.
    // In the future, we want to improve libp2p to make this easier to do.
    #[derive(NetworkBehaviour)]
    struct MyBehaviour<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite> {
        floodsub: libp2p::floodsub::Floodsub<TSubstream>,
        //mdns: libp2p::mdns::Mdns<TSubstream>,
    }

    impl<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite> libp2p::core::swarm::NetworkBehaviourEventProcess<void::Void> for MyBehaviour<TSubstream> {
        fn inject_event(&mut self, _ev: void::Void) {
            void::unreachable(_ev)
        }
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
            floodsub: libp2p::floodsub::Floodsub::new(public_key.clone().into_peer_id()),
            //mdns: libp2p::mdns::Mdns::new()?
        };

        behaviour.floodsub.subscribe(floodsub_topic.clone());
        libp2p::Swarm::new(transport, behaviour, libp2p::core::topology::MemoryTopology::empty(public_key))
    };

    // Listen on all interfaces and whatever port the OS assigns
    let addr = libp2p::Swarm::listen_on(&mut swarm, "/ip4/127.0.0.1/udp/0/quic".parse()?)?;
    println!("Listening on {:?}", addr);

    // Reach out to another node if specified
    if let Some(to_dial) = args.dial {
        let dialing = to_dial.clone();
        match to_dial.parse() {
            Ok(to_dial) => {
                match libp2p::Swarm::dial_addr(&mut swarm, to_dial) {
                    Ok(_) => println!("Dialed {:?}", dialing),
                    Err(e) => println!("Dial {:?} failed: {:?}", dialing, e)
                }
            },
            Err(err) => println!("Failed to parse address to dial: {:?}", err),
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
                Async::NotReady => break,
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

