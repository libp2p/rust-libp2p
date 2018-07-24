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

pub extern crate bytes;
pub extern crate futures;
#[cfg(not(target_os = "emscripten"))]
pub extern crate tokio_current_thread;
pub extern crate multiaddr;
pub extern crate tokio_io;
pub extern crate tokio_codec;

pub extern crate libp2p_core as core;
#[cfg(not(target_os = "emscripten"))]
pub extern crate libp2p_dns as dns;
pub extern crate libp2p_identify as identify;
pub extern crate libp2p_kad as kad;
pub extern crate libp2p_floodsub as floodsub;
pub extern crate libp2p_mplex as mplex;
pub extern crate libp2p_peerstore as peerstore;
pub extern crate libp2p_ping as ping;
pub extern crate libp2p_ratelimit as ratelimit;
pub extern crate libp2p_relay as relay;
#[cfg(all(not(target_os = "emscripten"), feature = "libp2p-secio"))]
pub extern crate libp2p_secio as secio;
#[cfg(not(target_os = "emscripten"))]
pub extern crate libp2p_tcp_transport as tcp;
pub extern crate libp2p_transport_timeout as transport_timeout;
pub extern crate libp2p_websocket as websocket;
pub extern crate libp2p_yamux as yamux;

pub mod simple;

pub use self::core::{Transport, ConnectionUpgrade, PeerId, swarm};
pub use self::multiaddr::Multiaddr;
pub use self::simple::SimpleProtocol;
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

#[cfg(not(target_os = "emscripten"))]
pub type InnerImplementation = core::transport::OrTransport<dns::DnsConfig<tcp::TcpConfig>, websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>>>;
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
        let tcp = tcp::TcpConfig::new();
        let with_dns = dns::DnsConfig::new(tcp);
        let with_ws = websocket::WsConfig::new(with_dns.clone());
        let inner = with_dns.or_transport(with_ws);

        CommonTransport {
            inner: CommonTransportInner { inner }
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
    type MultiaddrFuture = <InnerImplementation as Transport>::MultiaddrFuture;
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
