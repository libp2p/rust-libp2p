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

//! Libp2p is a peer-to-peer framework.
//!
//! # Major libp2p concepts
//!
//! Here is a list of all the major concepts of libp2p.
//!
//! ## Multiaddr
//!
//! A `Multiaddr` is a way to reach a node. Examples:
//!
//! * `/ip4/80.123.90.4/tcp/5432`
//! * `/ip6/[::1]/udp/10560/quic`
//! * `/unix//path/to/socket`
//!
//! ## Transport
//!
//! `Transport` is a trait that represents an object capable of dialing multiaddresses or
//! listening on multiaddresses. The `Transport` produces an output which varies depending on the
//! object that implements the trait.
//!
//! Each implementation of `Transport` typically supports only some multiaddresses. For example
//! the `TcpConfig` type (which implements `Transport`) only supports multiaddresses of the format
//! `/ip4/.../tcp/...`.
//!
//! Example:
//!
//! ```rust
//! use libp2p::{Multiaddr, Transport, tcp::TcpConfig};
//! let tcp_transport = TcpConfig::new();
//! let addr: Multiaddr = "/ip4/98.97.96.95/tcp/20500".parse().expect("invalid multiaddr");
//! let _outgoing_connec = tcp_transport.dial(addr);
//! // Note that `_outgoing_connec` is a `Future`, and therefore doesn't do anything by itself
//! // unless it is run through a tokio runtime.
//! ```
//!
//! The easiest way to create a transport is to use the `CommonTransport` struct. This struct
//! provides support for the most common protocols.
//!
//! Example:
//!
//! ```rust
//! use libp2p::CommonTransport;
//! let _transport = CommonTransport::new();
//! // _transport.dial(...);
//! ```
//!
//! See the documentation of the `libp2p-core` crate for more details about transports.
//!
//! # Connection upgrades
//!
//! Once a connection has been opened with a remote through a `Transport`, it can be *upgraded*.
//! This consists in negotiating a protocol with the remote (through a negotiation protocol
//! `multistream-select`), and applying that protocol on the socket.
//!
//! Example:
//!
//! ```rust
//! # #[cfg(all(not(target_os = "emscripten"), feature = "libp2p-secio"))] {
//! use libp2p::{Transport, tcp::TcpConfig, secio::{SecioConfig, SecioKeyPair}};
//! let tcp_transport = TcpConfig::new();
//! let secio_upgrade = SecioConfig::new(SecioKeyPair::ed25519_generated().unwrap());
//! let with_security = tcp_transport.with_upgrade(secio_upgrade);
//! // let _ = with_security.dial(...);
//! // `with_security` also implements the `Transport` trait, and all the connections opened
//! // through it will automatically negotiate the `secio` protocol.
//! # }
//! ```
//!
//! See the documentation of the `libp2p-core` crate for more details about upgrades.
//!
//! ## Topology
//!
//! The `Topology` trait is implemented for types that hold the layout of a network. When other
//! components need the network layout to operate, they are passed an instance of a `Topology`.
//!
//! The most basic implementation of `Topology` is the `MemoryTopology`, which is essentially a
//! `HashMap`. Creating your own `Topology` makes it possible to add for example a reputation
//! system.
//!
//! ## Network behaviour
//!
//! The `NetworkBehaviour` trait is implemented on types that provide some capability to the
//! network. Examples of network behaviours include: periodically ping the nodes we are connected
//! to, periodically ask for information from the nodes we are connected to, connect to a DHT and
//! make queries to it, propagate messages to the nodes we are connected to (pubsub), and so on.
//!
//! ## Swarm
//!
//! The `Swarm` struct contains all active and pending connections to remotes and manages the
//! state of all the substreams that have been opened, and all the upgrades that were built upon
//! these substreams.
//! 
//! It combines a `Transport`, a `NetworkBehaviour` and a `Topology` together.
//!
//! See the documentation of the `libp2p-core` crate for more details about creating a swarm.
//!
//! # Using libp2p
//!
//! This section contains details about how to use libp2p in practice.
//!
//! The most simple way to use libp2p consists in the following steps:
//!
//! - Create a *base* implementation of `Transport` that combines all the protocols you want and
//!   the upgrades you want, such as the security layer and multiplexing.
//! - Create a struct that implements the `NetworkBehaviour` trait and that combines all the
//!   network behaviours that you want.
//! - Create and implement the `Topology` trait that to store the topology of the network.
//! - Create a swarm that combines your base transport, the network behaviour, and the topology.
//! - This swarm can now be polled with the `tokio` library in order to start the network.
//!

pub extern crate bytes;
pub extern crate futures;
pub extern crate multiaddr;
pub extern crate multihash;
pub extern crate tokio_io;
pub extern crate tokio_codec;

extern crate libp2p_core_derive;
extern crate tokio_executor;

pub extern crate libp2p_core as core;
#[cfg(not(target_os = "emscripten"))]
pub extern crate libp2p_dns as dns;
pub extern crate libp2p_identify as identify;
pub extern crate libp2p_kad as kad;
pub extern crate libp2p_floodsub as floodsub;
pub extern crate libp2p_mplex as mplex;
#[cfg(not(target_os = "emscripten"))]
pub extern crate libp2p_mdns as mdns;
pub extern crate libp2p_peerstore as peerstore;
pub extern crate libp2p_ping as ping;
pub extern crate libp2p_plaintext as plaintext;
pub extern crate libp2p_ratelimit as ratelimit;
pub extern crate libp2p_relay as relay;
pub extern crate libp2p_secio as secio;
#[cfg(not(target_os = "emscripten"))]
pub extern crate libp2p_tcp_transport as tcp;
pub extern crate libp2p_transport_timeout as transport_timeout;
pub extern crate libp2p_uds as uds;
#[cfg(feature = "libp2p-websocket")]
pub extern crate libp2p_websocket as websocket;
pub extern crate libp2p_yamux as yamux;

mod transport_ext;

pub mod simple;

pub use self::core::{
    Transport, PeerId, Swarm,
    upgrade::{InboundUpgrade, InboundUpgradeExt, OutboundUpgrade, OutboundUpgradeExt}
};
pub use libp2p_core_derive::NetworkBehaviour;
pub use self::multiaddr::Multiaddr;
pub use self::simple::SimpleProtocol;
pub use self::transport_ext::TransportExt;
pub use self::transport_timeout::TransportTimeout;

/// Implementation of `Transport` that supports the most common protocols.
///
/// The list currently is TCP/IP, DNS, and WebSockets. However this list could change in the
/// future to get new transports.
#[derive(Debug, Clone)]
pub struct CommonTransport {
    // The actual implementation of everything.
    inner: CommonTransportInner
}

#[cfg(all(not(target_os = "emscripten"), feature = "libp2p-websocket"))]
pub type InnerImplementation = core::transport::OrTransport<dns::DnsConfig<tcp::TcpConfig>, websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>>>;
#[cfg(all(not(target_os = "emscripten"), not(feature = "libp2p-websocket")))]
pub type InnerImplementation = dns::DnsConfig<tcp::TcpConfig>;
#[cfg(target_os = "emscripten")]
pub type InnerImplementation = websocket::BrowserWsConfig;

#[derive(Debug, Clone)]
struct CommonTransportInner {
    inner: InnerImplementation,
}

impl CommonTransport {
    /// Initializes the `CommonTransport`.
    #[inline]
    #[cfg(not(target_os = "emscripten"))]
    pub fn new() -> CommonTransport {
        let transport = tcp::TcpConfig::new();
        let transport = dns::DnsConfig::new(transport);
        #[cfg(feature = "libp2p-websocket")]
        let transport = {
            let trans_clone = transport.clone();
            transport.or_transport(websocket::WsConfig::new(trans_clone))
        };

        CommonTransport {
            inner: CommonTransportInner { inner: transport }
        }
    }

    /// Initializes the `CommonTransport`.
    #[inline]
    #[cfg(target_os = "emscripten")]
    pub fn new() -> CommonTransport {
        let inner = websocket::BrowserWsConfig::new();
        CommonTransport {
            inner: CommonTransportInner { inner: inner }
        }
    }
}

impl Transport for CommonTransport {
    type Output = <InnerImplementation as Transport>::Output;
    type Listener = <InnerImplementation as Transport>::Listener;
    type ListenerUpgrade = <InnerImplementation as Transport>::ListenerUpgrade;
    type Dial = <InnerImplementation as Transport>::Dial;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        match self.inner.inner.listen_on(addr) {
            Ok(res) => Ok(res),
            Err((inner, addr)) => {
                let trans = CommonTransport { inner: CommonTransportInner { inner: inner } };
                Err((trans, addr))
            }
        }
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match self.inner.inner.dial(addr) {
            Ok(res) => Ok(res),
            Err((inner, addr)) => {
                let trans = CommonTransport { inner: CommonTransportInner { inner: inner } };
                Err((trans, addr))
            }
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.inner.nat_traversal(server, observed)
    }
}

/// The `multiaddr!` macro is an easy way for a user to create a `Multiaddr`.
///
/// Example:
///
/// ```rust
/// # #[macro_use]
/// # extern crate libp2p;
/// # fn main() {
/// let _addr = multiaddr![Ip4([127, 0, 0, 1]), Tcp(10500u16)];
/// # }
/// ```
///
/// Each element passed to `multiaddr![]` should be a variant of the `Protocol` enum. The
/// optional parameter is casted into the proper type with the `Into` trait.
///
/// For example, `Ip4([127, 0, 0, 1])` works because `Ipv4Addr` implements `From<[u8; 4]>`.
#[macro_export]
macro_rules! multiaddr {
    ($($comp:ident $(($param:expr))*),+) => {
        {
            use std::iter;
            let elem = iter::empty::<$crate::multiaddr::Protocol>();
            $(
                let elem = {
                    let cmp = $crate::multiaddr::Protocol::$comp $(( $param.into() ))*;
                    elem.chain(iter::once(cmp))
                };
            )+
            elem.collect::<$crate::Multiaddr>()
        }
    }
}
