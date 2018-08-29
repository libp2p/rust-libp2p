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

//! Example runs
//! ============
//!
//! As destination
//! --------------
//!
//! relay listen \
//!     --self "QmcwnUP8cM2U4EeMW6g6nbFUQRyE6xXh65TPaZD9bqkhdK" \
//!     --listen "/ip4/127.0.0.1/tcp/10003" \
//!     --peer "QmXnxVaQoP8cPm2J5uN73GPEu3pCkJdYDNDCMZS8dMxTXL=/ip4/127.0.0.1/tcp/10002"
//!
//! As relay
//! --------
//!
//! relay listen \
//!     --self "QmXnxVaQoP8cPm2J5uN73GPEu3pCkJdYDNDCMZS8dMxTXL" \
//!     --listen "/ip4/127.0.0.1/tcp/10002" \
//!     --peer "QmcwnUP8cM2U4EeMW6g6nbFUQRyE6xXh65TPaZD9bqkhdK=/ip4/127.0.0.1/tcp/10003"
//!
//! As source
//! ---------
//!
//! relay dial \
//!     --self "QmYJ46WjbwxLkrTGU1JZNEN3HnYbcuES8QahG1PAMCxFY8" \
//!     --destination "QmcwnUP8cM2U4EeMW6g6nbFUQRyE6xXh65TPaZD9bqkhdK" \
//!     --relay "QmXnxVaQoP8cPm2J5uN73GPEu3pCkJdYDNDCMZS8dMxTXL" \
//!     --peer "QmXnxVaQoP8cPm2J5uN73GPEu3pCkJdYDNDCMZS8dMxTXL=/ip4/127.0.0.1/tcp/10002"

extern crate bytes;
extern crate env_logger;
extern crate futures;
extern crate libp2p;
extern crate libp2p_yamux;
extern crate rand;
#[macro_use]
extern crate structopt;
extern crate tokio_codec;
extern crate tokio_current_thread;

use libp2p::SimpleProtocol;
use libp2p::core::Multiaddr;
use libp2p::core::transport::Transport;
use libp2p::core::upgrade;
use futures::{future::{self, Either, Loop, loop_fn}, prelude::*};
use libp2p::peerstore::{PeerAccess, PeerId, Peerstore, memory_peerstore::MemoryPeerstore};
use libp2p::relay::{RelayConfig, RelayTransport};
use std::{error::Error, iter, str::FromStr, sync::Arc, time::Duration};
use structopt::StructOpt;
use libp2p::tcp::TcpConfig;
use tokio_codec::{BytesCodec, Framed};

fn main() -> Result<(), Box<Error>> {
    env_logger::init();
    match Options::from_args() {
        Options::Dialer(opts) => run_dialer(opts),
        Options::Listener(opts) => run_listener(opts)
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "relay", about = "Usage example for /libp2p/relay/circuit/0.1.0")]
enum Options {
    #[structopt(name = "dial")]
    /// Run in dial mode.
    Dialer(DialerOpts),
    #[structopt(name = "listen")]
    /// Run in listener mode.
    Listener(ListenerOpts)
}

#[derive(Debug, StructOpt)]
struct DialerOpts {
    #[structopt(short = "s", long = "self", parse(try_from_str))]
    /// The PeerId of this node.
    me: PeerId,
    #[structopt(short = "d", long = "destination", parse(try_from_str))]
    /// The PeerId to dial.
    dest: PeerId,
    #[structopt(short = "r", long = "relay", parse(try_from_str))]
    /// The PeerId of the relay node to use when dialing the destination.
    relay: PeerId,
    #[structopt(short = "p", long = "peer", parse(try_from_str = "parse_peer_addr"))]
    /// A network peer known to this node (format: PeerId=Multiaddr).
    /// For example: QmXnxVaQoP8cPm2J5uN73GPEu3pCkJdYDNDCMZS8dMxTXL=/ip4/127.0.0.1/tcp/12345
    peers: Vec<(PeerId, Multiaddr)>
}

#[derive(Debug, StructOpt)]
struct ListenerOpts {
    #[structopt(short = "s", long = "self", parse(try_from_str))]
    /// The PeerId of this node.
    me: PeerId,
    #[structopt(short = "p", long = "peer", parse(try_from_str = "parse_peer_addr"))]
    /// A network peer know to this node (format: PeerId=Multiaddr).
    /// For example: QmXnxVaQoP8cPm2J5uN73GPEu3pCkJdYDNDCMZS8dMxTXL=/ip4/127.0.0.1/tcp/12345
    peers: Vec<(PeerId, Multiaddr)>,
    #[structopt(short = "l", long = "listen", parse(try_from_str))]
    /// The multiaddress to listen for incoming connections.
    listen: Multiaddr
}

fn run_dialer(opts: DialerOpts) -> Result<(), Box<Error>> {
    let store = Arc::new(MemoryPeerstore::empty());
    for (p, a) in opts.peers {
        store.peer_or_create(&p).add_addr(a, Duration::from_secs(600))
    }

    let transport = {
        let tcp = TcpConfig::new()
            .with_upgrade(libp2p_yamux::Config::default())
            .into_connection_reuse();
        RelayTransport::new(opts.me, tcp, store, iter::once(opts.relay)).with_dummy_muxing()
    };

    let echo = SimpleProtocol::new("/echo/1.0.0", |socket| {
        Ok(Framed::new(socket, BytesCodec::new()))
    });

    let (control, future) = libp2p::core::swarm(transport.clone().with_upgrade(echo.clone()), |socket, _| {
        println!("sending \"hello world\"");
        socket.send("hello world".into())
            .and_then(|socket| socket.into_future().map_err(|(e, _)| e).map(|(m, _)| m))
            .and_then(|message| {
                println!("received message: {:?}", message);
                Ok(())
            })
    });

    let address = format!("/p2p-circuit/p2p/{}", opts.dest.to_base58()).parse()?;

    control.dial(address, transport.with_upgrade(echo)).map_err(|_| "failed to dial")?;

    tokio_current_thread::block_on_all(future).map_err(From::from)
}

fn run_listener(opts: ListenerOpts) -> Result<(), Box<Error>> {
    let store = Arc::new(MemoryPeerstore::empty());
    for (p, a) in opts.peers {
        store.peer_or_create(&p).add_addr(a, Duration::from_secs(600))
    }

    let transport = TcpConfig::new()
        .with_upgrade(libp2p_yamux::Config::default())
        .into_connection_reuse();

    let relay = RelayConfig::new(opts.me, transport.clone(), store);

    let echo = SimpleProtocol::new("/echo/1.0.0", |socket| {
        Ok(Framed::new(socket, BytesCodec::new()))
    });

    let upgraded = transport.with_upgrade(relay)
        .and_then(|out, endpoint, addr| {
            match out {
                libp2p::relay::Output::Sealed(future) => {
                    Either::A(future.map(|out| (Either::A(out), Either::A(addr))))
                }
                libp2p::relay::Output::Stream(socket) => {
                    Either::B(upgrade::apply(socket, echo, endpoint, addr)
                        .map(|(out, addr)| (Either::B(out), Either::B(addr))))
                }
            }
        });

    let (control, future) = libp2p::core::swarm(upgraded, |out, _| {
        match out {
            Either::A(()) => Either::A(future::ok(())),
            Either::B(socket) => Either::B(loop_fn(socket, move |socket| {
                socket.into_future()
                    .map_err(|(e, _)| e)
                    .and_then(move |(msg, socket)| {
                        if let Some(msg) = msg {
                            println!("received at destination: {:?}", msg);
                            Either::A(socket.send(msg.freeze()).map(Loop::Continue))
                        } else {
                            println!("received EOF at destination");
                            Either::B(future::ok(Loop::Break(())))
                        }
                    })
            }))
        }
    });

    control.listen_on(opts.listen).map_err(|_| "failed to listen")?;
    tokio_current_thread::block_on_all(future).map_err(From::from)
}

// Custom parsers ///////////////////////////////////////////////////////////

fn parse_peer_addr(s: &str) -> Result<(PeerId, Multiaddr), Box<Error>> {
    let mut iter = s.splitn(2, '=');
    let p = iter.next()
        .and_then(|s| PeerId::from_str(s).ok())
        .ok_or("missing or invalid PeerId")?;
    let m = iter.next()
        .and_then(|s| Multiaddr::from_str(s).ok())
        .ok_or("missing or invalid Multiaddr")?;
    Ok((p, m))
}

