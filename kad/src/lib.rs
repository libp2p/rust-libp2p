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

//! Kademlia protocol. Allows peer discovery, records store and records fetch.
//!
//! # Usage
//!
//! Usage is done in the following steps:
//!
//! - Build a `KademliaConfig` that contains the way you want the Kademlia protocol to behave.
//!
//! - Build a `KademliaControllerPrototype` from that configuration object.
//!
//! - Build a `KademliaUpgrade` from that prototype. Then create a swarm (from the *swarm* crate)
//!   and pass the `KademliaUpgrade` you built as part of the list of protocols.
//!
//! - Then turn the controller prototype into an actual `KademliaController` by passing to it the
//!   swarm controller you got.
//!
//! - You can now perform operations using that controller.
//!

// TODO: we allow dead_code for now because this library contains a lot of unused code that will
//       be useful later for record store
#![allow(dead_code)]

// # Crate organization
//
// The crate contains three levels of abstractions over the Kademlia protocol.
//
// - The first level of abstraction is in `protocol`. The API of this module lets you turn a raw
//   bytes stream (`AsyncRead + AsyncWrite`) into a `Sink + Stream` of raw but strongly-typed
//   Kademlia messages.
//
// - The second level of abstraction is in `kad_server`. Its API lets you upgrade a connection and
//   obtain a future (that must be driven to completion), plus a controller. Processing the future
//   will automatically respond to Kad requests received by the remote. The controller lets you
//   send your own requests to this remote and obtain strongly-typed responses.
//
// - The third level of abstraction is in `high_level`. This module also provides a
//   `ConnectionUpgrade`, but all the upgraded connections share informations through a struct in
//   an `Arc`. The user has a single clonable controller that operates on all the upgraded
//   connections. This controller lets you perform peer discovery and record load operations over
//   the whole network.
//

extern crate arrayvec;
extern crate bigint;
extern crate bs58;
extern crate bytes;
extern crate datastore;
extern crate fnv;
extern crate futures;
extern crate libp2p_identify;
extern crate libp2p_ping;
extern crate libp2p_core;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate parking_lot;
extern crate protobuf;
extern crate rand;
extern crate smallvec;
extern crate tokio_io;
extern crate tokio_timer;
extern crate varint;

pub use self::high_level::{KademliaConfig, KademliaController, KademliaControllerPrototype};
pub use self::high_level::{KademliaPeerReqStream, KademliaUpgrade, KademliaPeerReq};
pub use self::protocol::{ConnectionType, Peer};
pub use self::query::QueryEvent;

mod high_level;
mod kad_server;
mod kbucket;
mod protobuf_structs;
mod protocol;
mod query;
