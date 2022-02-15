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
use lazy_static::lazy_static;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

mod behaviour;

pub use crate::behaviour::{Mdns, MdnsEvent};

/// The DNS service name for all libp2p peers used to query for addresses.
const SERVICE_NAME: &[u8] = b"_p2p._udp.local";
/// The meta query for looking up the `SERVICE_NAME`.
const META_QUERY_SERVICE: &[u8] = b"_services._dns-sd._udp.local";

lazy_static! {
    pub static ref IPV4_MDNS_MULTICAST_ADDRESS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
    pub static ref IPV6_MDNS_MULTICAST_ADDRESS: Ipv6Addr =
        Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0xFB);
}

/// Configuration for mDNS.
#[derive(Debug, Clone)]
pub struct MdnsConfig {
    /// TTL to use for mdns records.
    pub ttl: Duration,
    /// Interval at which to poll the network for new peers. This isn't
    /// necessary during normal operation but avoids the case that an
    /// initial packet was lost and not discovering any peers until a new
    /// peer joins the network. Receiving an mdns packet resets the timer
    /// preventing unnecessary traffic.
    pub query_interval: Duration,
    /// Use IPv6 instead of IPv4.
    pub enable_ipv6: bool,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(6 * 60),
            query_interval: Duration::from_secs(5 * 60),
            enable_ipv6: false,
        }
    }
}
