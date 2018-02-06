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

extern crate base58;
extern crate bigint;
extern crate bytes;
extern crate datastore;
extern crate fnv;
extern crate futures;
extern crate libp2p_identify;
extern crate libp2p_peerstore;
extern crate libp2p_ping;
extern crate libp2p_swarm;
extern crate multiaddr;
extern crate parking_lot;
extern crate protobuf;
extern crate rand;
extern crate smallvec;
extern crate tokio_io;
extern crate tokio_timer;
extern crate varint;

pub use self::high_level::{KademliaConfig, KademliaController, KademliaControllerPrototype};
pub use self::high_level::KademliaUpgrade;

mod high_level;
mod kad_server;
pub mod kbucket; // TODO: not pub
mod protobuf_structs;
mod protocol;
mod query;
