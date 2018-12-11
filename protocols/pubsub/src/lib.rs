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

//! A publish-subscribe peer-to-peer messaging protocol.
//! For a specification, see https://github.com/libp2p/specs/tree/master/pubsub.
//! Includes floodsub and gossipsub as protocols.

extern crate bs58;
extern crate bytes;
extern crate cuckoofilter;
extern crate fnv;
extern crate futures;
extern crate libp2p_core;
extern crate protobuf;
extern crate rand;
extern crate smallvec;
extern crate tokio_codec;
extern crate tokio_io;
extern crate unsigned_varint;

/// A flooding and subscribing p2p messaging protocol, i.e. dials and listens
/// on all peers of a topic. For more details, see the pubsub/floodsub
/// [spec](https://github.com/libp2p/specs/tree/master/pubsub) for details.
pub mod floodsub;
/// A gossiping and subscribing p2p messaging protocol, dials and listens on
/// a random subset of peers in a mesh network. For more details, see the gossipsub
/// [spec](https://github.com/libp2p/specs/tree/master/pubsub/gossipsub).
pub mod gossipsub;

pub use self::floodsub::layer::Floodsub;
pub use self::floodsub::topic::{Topic, TopicBuilder, TopicHash};
