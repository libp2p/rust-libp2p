// Copyright 2017 Parity Technologies (UK) Ltd.
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

// TODO: use this once stable ; for now we just copy-paste the content of the README.md
//#![doc(include = "../README.md")]

//! Transport and protocol upgrade system of *libp2p*.
//! 
//! This crate contains all the core traits and mechanisms of the transport system of *libp2p*.
//! 
//! # The `Transport` trait
//! 
//! The main trait that this crate provides is `Transport`, which provides the `dial` and
//! `listen_on` methods and can be used to dial or listen on a multiaddress. The `swarm` crate
//! itself does not provide any concrete (ie. non-dummy, non-adapter) implementation of this trait.
//! It is implemented on structs that are provided by external crates, such as `TcpConfig` from
//! `tcp-transport`, `UdpConfig`, or `WebsocketConfig` (note: as of the writing of this
//! documentation, the last two structs don't exist yet).
//! 
//! Each implementation of `Transport` only supports *some* multiaddress protocols, for example
//! the `TcpConfig` struct only supports multiaddresses that look like `/ip*/*.*.*.*/tcp/*`. It is
//! possible to group two implementations of `Transport` with the `or_transport` method, in order
//! to obtain a single object that supports the protocols of both objects at once. This can be done
//! multiple times in a row in order to chain as many implementations as you want.
//! 
//! // TODO: right now only tcp-transport exists, we need to add an example for chaining
//! //       multiple transports once that makes sense
//! 
//! ## The `MuxedTransport` trait
//! 
//! The `MuxedTransport` trait is an extension to the `Transport` trait, and is implemented on
//! transports that can receive incoming connections on streams that have been opened with `dial()`.
//! 
//! The trait provides the `dial_and_listen()` method, which returns both a dialer and a stream of
//! incoming connections.
//! 
//! > **Note**: This trait is mainly implemented for transports that provide stream muxing
//! >           capabilities.
//! 
//! # Connection upgrades
//! 
//! Once a socket has been opened with a remote through a `Transport`, it can be *upgraded*. This
//! consists in negotiating a protocol with the remote (through `multistream-select`), and applying
//! that protocol on the socket.
//! 
//! A potential connection upgrade is represented with the `ConnectionUpgrade` trait. The trait
//! consists in a protocol name plus a method that turns the socket into an `Output` object whose
//! nature and type is specific to each upgrade.
//! 
//! There exists three kinds of connection upgrades: middlewares, muxers, and actual protocols.
//! 
//! ## Middlewares
//! 
//! Examples of middleware connection upgrades include `PlainTextConfig` (dummy upgrade) or
//! `SecioConfig` (encyption layer, provided by the `secio` crate).
//! 
//! The output of a middleware connection upgrade implements the `AsyncRead` and `AsyncWrite`
//! traits, just like sockets do.
//! 
//! A middleware can be applied on a transport by using the `with_upgrade` method of the
//! `Transport` trait. The return value of this method also implements the `Transport` trait, which
//! means that you can call `dial()` and `listen_on()` on it in order to directly obtain an
//! upgraded connection or a listener that will yield upgraded connections. Similarly, the
//! `dial_and_listen()` method will automatically apply the upgrade on both the dialer and the
//! listener. An error is produced if the remote doesn't support the protocol corresponding to the
//! connection upgrade.
//! 
//! ```
//! extern crate libp2p_swarm;
//! extern crate libp2p_tcp_transport;
//! extern crate tokio_core;
//! 
//! use libp2p_swarm::Transport;
//! 
//! # fn main() {
//! let tokio_core = tokio_core::reactor::Core::new().unwrap();
//! let tcp_transport = libp2p_tcp_transport::TcpConfig::new(tokio_core.handle());
//! let upgraded = tcp_transport.with_upgrade(libp2p_swarm::PlainTextConfig);
//! 
//! // upgraded.dial(...)   // automatically applies the plain text protocol on the socket
//! # }
//! ```
//!
//! ## Muxers
//! 
//! The concept of *muxing* consists in using a single stream as if it was multiple substreams.
//! 
//! If the output of the connection upgrade instead implements the `StreamMuxer` and `Clone`
//! traits, then you can turn the `UpgradedNode` struct into a `ConnectionReuse` struct by calling
//! `ConnectionReuse::from(upgraded_node)`.
//! 
//! The `ConnectionReuse` struct then implements the `Transport` and `MuxedTransport` traits, and
//! can be used to dial or listen to multiaddresses, just like any other transport. The only
//! difference is that dialing a node will try to open a new substream on an existing connection
//! instead of opening a new one every time.
//! 
//! > **Note**: Right now the `ConnectionReuse` struct is not fully implemented.
//! 
//! TODO: add an example once the multiplex pull request is merged
//! 
//! ## Actual protocols
//! 
//! *Actual protocols* work the same way as middlewares, except that their `Output` doesn't
//! implement the `AsyncRead` and `AsyncWrite` traits. This means that that the return value of
//! `with_upgrade` does **not** implement the `Transport` trait and thus cannot be used as a
//! transport.
//! 
//! However the `UpgradedNode` struct returned by `with_upgrade` still provides methods named
//! `dial`, `listen_on`, and `dial_and_listen`, which will yield you a `Future` or a `Stream`,
//! which you can use to obtain the `Output`. This `Output` can then be used in a protocol-specific
//! way to use the protocol.
//! 
//! ```no_run
//! extern crate futures;
//! extern crate libp2p_ping;
//! extern crate libp2p_swarm;
//! extern crate libp2p_tcp_transport;
//! extern crate tokio_core;
//! 
//! use futures::Future;
//! use libp2p_ping::Ping;
//! use libp2p_swarm::Transport;
//! 
//! # fn main() {
//! let mut core = tokio_core::reactor::Core::new().unwrap();
//! 
//! let ping_finished_future = libp2p_tcp_transport::TcpConfig::new(core.handle())
//!     // We have a `TcpConfig` struct that implements `Transport`, and apply a `Ping` upgrade on it.
//!     .with_upgrade(Ping)
//!     // TODO: right now the only available protocol is ping, but we want to replace it with
//!     //       something that is more simple to use
//!     .dial(libp2p_swarm::Multiaddr::new("127.0.0.1:12345").unwrap()).unwrap_or_else(|_| panic!())
//!     .and_then(|(mut pinger, service)| {
//!         pinger.ping().map_err(|_| panic!()).select(service).map_err(|_| panic!())
//!     });
//! 
//! // Runs until the ping arrives.
//! core.run(ping_finished_future).unwrap();
//! # }
//! ```
//! 
//! ## Grouping protocols
//! 
//! You can use the `.or_upgrade()` method to group multiple upgrades together. The return value
//! also implements the `ConnectionUpgrade` trait and will choose one of the protocols amongst the
//! ones supported.
//!

extern crate bytes;
#[macro_use]
extern crate futures;
extern crate multistream_select;
extern crate parking_lot;
extern crate smallvec;
extern crate tokio_io;

/// Multi-address re-export.
pub extern crate multiaddr;

mod connection_reuse;
pub mod muxing;
pub mod transport;

pub use self::connection_reuse::ConnectionReuse;
pub use self::multiaddr::Multiaddr;
pub use self::muxing::StreamMuxer;
pub use self::transport::{ConnectionUpgrade, PlainTextConfig, Transport, UpgradedNode, OrUpgrade};
pub use self::transport::{Endpoint, SimpleProtocol, MuxedTransport, UpgradeExt};
