// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Handles the `/ipfs/ping/1.0.0` protocol. This allows pinging a remote node and waiting for an
//! answer.
//!
//! # Usage
//!
//! Create a `Ping` struct, which implements the `ConnectionUpgrade` trait. When used as a
//! connection upgrade, it will produce a tuple of type `(Pinger, impl Future<Item = ()>)` which
//! are named the *pinger* and the *ponger*.
//!
//! The *pinger* has a method named `ping` which will send a ping to the remote, while the *ponger*
//! is a future that will process the data received on the socket and will be signalled only when
//! the connection closes.
//!
//! # About timeouts
//!
//! For technical reasons, this crate doesn't handle timeouts. The action of pinging returns a
//! future that is signalled only when the remote answers. If the remote is not responsive, the
//! future will never be signalled.
//!
//! For implementation reasons, resources allocated for a ping are only ever fully reclaimed after
//! a pong has been received by the remote. Therefore if you repeatidely ping a non-responsive
//! remote you will end up using more and more memory (albeit the amount is very very small every
//! time), even if you destroy the future returned by `ping`.
//!
//! This is probably not a problem in practice, because the nature of the ping protocol is to
//! determine whether a remote is still alive, and any reasonable user of this crate will close
//! connections to non-responsive remotes.
//!
//! # Example
//!
//! ```no_run
//! extern crate futures;
//! extern crate libp2p_ping;
//! extern crate libp2p_core;
//! extern crate libp2p_tcp_transport;
//! extern crate tokio;
//!
//! use futures::{Future, Stream};
//! use libp2p_core::{transport::Transport, upgrade::apply_outbound};
//! use libp2p_ping::protocol::Ping;
//! use tokio::runtime::current_thread::Runtime;
//!
//! # fn main() {
//! let ping_dialer = libp2p_tcp_transport::TcpConfig::new()
//!     .and_then(|socket, _| {
//!         apply_outbound(socket, Ping::default()).map_err(|e| e.into_io_error())
//!     })
//!     .dial("/ip4/127.0.0.1/tcp/12345".parse::<libp2p_core::Multiaddr>().unwrap()).unwrap_or_else(|_| panic!())
//!     .and_then(|mut pinger| {
//!         pinger.ping(());
//!         let f = pinger.into_future()
//!             .map(|_| println!("received pong"))
//!             .map_err(|(e, _)| e);
//!         Box::new(f) as Box<Future<Item = _, Error = _> + Send>
//!     });
//!
//! // Runs until the ping arrives.
//! let mut rt = Runtime::new().unwrap();
//! let _ = rt.block_on(ping_dialer).unwrap();
//! # }
//! ```
//!

extern crate arrayvec;
extern crate bytes;
extern crate futures;
extern crate libp2p_core;
extern crate log;
extern crate multistream_select;
extern crate parking_lot;
extern crate rand;
extern crate tokio_codec;
extern crate tokio_io;
extern crate tokio_timer;
extern crate void;

pub use self::dial_handler::PeriodicPingHandler;
pub use self::dial_layer::PeriodicPingBehaviour;
pub use self::listen_handler::PingListenHandler;
pub use self::listen_layer::PingListenBehaviour;

pub mod protocol;

mod dial_handler;
mod dial_layer;
mod listen_handler;
mod listen_layer;
