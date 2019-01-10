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

//! mDNS is a protocol defined by [RFC 6762](https://tools.ietf.org/html/rfc6762) that allows
//! querying nodes that correspond to a certain domain name.
//!
//! In the context of libp2p, the mDNS protocol is used to discover other nodes on the local
//! network that support libp2p.
//!
//! # Usage
//!
//! This crate provides the `Mdns` struct which implements the `NetworkBehaviour` trait. This
//! struct will automatically discover other libp2p nodes on the local network.
//!

extern crate data_encoding;
extern crate dns_parser;
extern crate futures;
extern crate libp2p_core;
extern crate multiaddr;
extern crate net2;
extern crate rand;
extern crate smallvec;
extern crate tokio_io;
extern crate tokio_reactor;
extern crate tokio_timer;
extern crate tokio_udp;
extern crate void;

#[cfg(test)]
extern crate tokio;

/// Hardcoded name of the mDNS service. Part of the mDNS libp2p specifications.
const SERVICE_NAME: &'static [u8] = b"_p2p._udp.local";
/// Hardcoded name of the service used for DNS-SD.
const META_QUERY_SERVICE: &'static [u8] = b"_services._dns-sd._udp.local";

pub use self::behaviour::{Mdns, MdnsTopology};
pub use self::service::MdnsService;

mod behaviour;
mod dns;

pub mod service;
