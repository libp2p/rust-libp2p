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
//! let tcp = TcpConfig::new();
//! let addr: Multiaddr = "/ip4/98.97.96.95/tcp/20500".parse().expect("invalid multiaddr");
//! let _outgoing_connec = tcp.dial(addr);
//! // Note that `_outgoing_connec` is a `Future`, and therefore doesn't do anything by itself
//! // unless it is run through a tokio runtime.
//! ```
//!
//! The easiest way to create a transport is to use the `build_development_transport` function.
//! This function provides support for the most common protocols.
//!
//! Example:
//!
//! ```rust
//! let key = libp2p::identity::Keypair::generate_ed25519();
//! let _transport = libp2p::build_development_transport(key);
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
//! # #[cfg(all(any(target_os = "emscripten", target_os = "unknown"), feature = "libp2p-secio"))] {
//! use libp2p::{Transport, tcp::TcpConfig, secio::{SecioConfig, SecioKeyPair}};
//! let tcp = TcpConfig::new();
//! let secio_upgrade = SecioConfig::new(SecioKeyPair::ed25519_generated().unwrap());
//! let with_security = tcp.with_upgrade(secio_upgrade);
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

#![doc(html_logo_url = "https://libp2p.io/img/logo_small.png")]
#![doc(html_favicon_url = "https://libp2p.io/img/favicon.png")]

pub use bytes;
pub use futures;
#[doc(inline)]
pub use multiaddr;
#[doc(inline)]
pub use multihash;
pub use tokio_io;
pub use tokio_codec;

#[doc(inline)]
pub use libp2p_core as core;
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
#[doc(inline)]
pub use libp2p_dns as dns;
#[doc(inline)]
pub use libp2p_identify as identify;
#[doc(inline)]
pub use libp2p_kad as kad;
#[doc(inline)]
pub use libp2p_floodsub as floodsub;
#[doc(inline)]
pub use libp2p_mplex as mplex;
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
#[doc(inline)]
pub use libp2p_mdns as mdns;
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
#[doc(inline)]
pub use libp2p_noise as noise;
#[doc(inline)]
pub use libp2p_ping as ping;
#[doc(inline)]
pub use libp2p_plaintext as plaintext;
#[doc(inline)]
pub use libp2p_ratelimit as ratelimit;
#[doc(inline)]
pub use libp2p_secio as secio;
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
#[doc(inline)]
pub use libp2p_tcp as tcp;
#[doc(inline)]
pub use libp2p_uds as uds;
#[cfg(feature = "libp2p-websocket")]
#[doc(inline)]
pub use libp2p_websocket as websocket;
#[doc(inline)]
pub use libp2p_yamux as yamux;

mod transport_ext;

pub mod bandwidth;
pub mod simple;

pub use self::core::{
    identity,
    Transport, PeerId, Swarm,
    transport::TransportError,
    upgrade::{InboundUpgrade, InboundUpgradeExt, OutboundUpgrade, OutboundUpgradeExt}
};
pub use libp2p_core_derive::NetworkBehaviour;
pub use self::multiaddr::{Multiaddr, multiaddr as build_multiaddr};
pub use self::simple::SimpleProtocol;
pub use self::transport_ext::TransportExt;

use futures::prelude::*;
use std::{error, time::Duration};

/// Builds a `Transport` that supports the most commonly-used protocols that libp2p supports.
///
/// > **Note**: This `Transport` is not suitable for production usage, as its implementation
/// >           reserves the right to support additional protocols or remove deprecated protocols.
#[inline]
pub fn build_development_transport(keypair: identity::Keypair)
    -> impl Transport<Output = (PeerId, impl core::muxing::StreamMuxer<OutboundSubstream = impl Send, Substream = impl Send> + Send + Sync), Error = impl error::Error + Send, Listener = impl Send, Dial = impl Send, ListenerUpgrade = impl Send> + Clone
{
     build_tcp_ws_secio_mplex_yamux(keypair)
}

/// Builds an implementation of `Transport` that is suitable for usage with the `Swarm`.
///
/// The implementation supports TCP/IP, WebSockets over TCP/IP, secio as the encryption layer,
/// and mplex or yamux as the multiplexing layer.
///
/// > **Note**: If you ever need to express the type of this `Transport`.
pub fn build_tcp_ws_secio_mplex_yamux(keypair: identity::Keypair)
    -> impl Transport<Output = (PeerId, impl core::muxing::StreamMuxer<OutboundSubstream = impl Send, Substream = impl Send> + Send + Sync), Error = impl error::Error + Send, Listener = impl Send, Dial = impl Send, ListenerUpgrade = impl Send> + Clone
{
    CommonTransport::new()
        .with_upgrade(secio::SecioConfig::new(keypair))
        .and_then(move |out, endpoint| {
            let peer_id = PeerId::from(out.remote_key);
            let peer_id2 = peer_id.clone();
            let upgrade = core::upgrade::SelectUpgrade::new(yamux::Config::default(), mplex::MplexConfig::new())
                // TODO: use a single `.map` instead of two maps
                .map_inbound(move |muxer| (peer_id, muxer))
                .map_outbound(move |muxer| (peer_id2, muxer));

            core::upgrade::apply(out.stream, upgrade, endpoint)
                .map(|(id, muxer)| (id, core::muxing::StreamMuxerBox::new(muxer)))
        })
        .with_timeout(Duration::from_secs(20))
}

/// Implementation of `Transport` that supports the most common protocols.
///
/// The list currently is TCP/IP, DNS, and WebSockets. However this list could change in the
/// future to get new transports.
#[derive(Debug, Clone)]
struct CommonTransport {
    // The actual implementation of everything.
    inner: CommonTransportInner
}

#[cfg(all(not(any(target_os = "emscripten", target_os = "unknown")), feature = "libp2p-websocket"))]
type InnerImplementation = core::transport::OrTransport<dns::DnsConfig<tcp::TcpConfig>, websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>>>;
#[cfg(all(not(any(target_os = "emscripten", target_os = "unknown")), not(feature = "libp2p-websocket")))]
type InnerImplementation = dns::DnsConfig<tcp::TcpConfig>;
#[cfg(all(any(target_os = "emscripten", target_os = "unknown"), feature = "libp2p-websocket"))]
type InnerImplementation = websocket::BrowserWsConfig;
#[cfg(all(any(target_os = "emscripten", target_os = "unknown"), not(feature = "libp2p-websocket")))]
type InnerImplementation = core::transport::dummy::DummyTransport;

#[derive(Debug, Clone)]
struct CommonTransportInner {
    inner: InnerImplementation,
}

impl CommonTransport {
    /// Initializes the `CommonTransport`.
    #[inline]
    #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
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
    #[cfg(all(any(target_os = "emscripten", target_os = "unknown"), feature = "libp2p-websocket"))]
    pub fn new() -> CommonTransport {
        let inner = websocket::BrowserWsConfig::new();
        CommonTransport {
            inner: CommonTransportInner { inner }
        }
    }

    /// Initializes the `CommonTransport`.
    #[inline]
    #[cfg(all(any(target_os = "emscripten", target_os = "unknown"), not(feature = "libp2p-websocket")))]
    pub fn new() -> CommonTransport {
        let inner = core::transport::dummy::DummyTransport::new();
        CommonTransport {
            inner: CommonTransportInner { inner }
        }
    }
}

impl Transport for CommonTransport {
    type Output = <InnerImplementation as Transport>::Output;
    type Error = <InnerImplementation as Transport>::Error;
    type Listener = <InnerImplementation as Transport>::Listener;
    type ListenerUpgrade = <InnerImplementation as Transport>::ListenerUpgrade;
    type Dial = <InnerImplementation as Transport>::Dial;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        self.inner.inner.listen_on(addr)
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner.inner.dial(addr)
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.inner.nat_traversal(server, observed)
    }
}
