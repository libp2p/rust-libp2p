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
pub extern crate tokio_core;
pub extern crate multiaddr;

pub extern crate libp2p_core as core;
pub extern crate libp2p_dns as dns;
pub extern crate libp2p_identify as identify;
pub extern crate libp2p_kad as kad;
pub extern crate libp2p_floodsub as floodsub;
pub extern crate libp2p_mplex as mplex;
pub extern crate libp2p_peerstore as peerstore;
pub extern crate libp2p_ping as ping;
//pub extern crate libp2p_secio as secio;
pub extern crate libp2p_tcp_transport as tcp;
pub extern crate libp2p_websocket as websocket;

pub use self::core::{Transport, ConnectionUpgrade, swarm};
pub use self::multiaddr::Multiaddr;
pub use self::peerstore::PeerId;

/// Implementation of `Transport` that supports the most common protocols.
///
/// The list currently is TCP/IP, DNS, and WebSockets. However this list could change in the
/// future to get new transports.
// TODO: handle the emscripten situation, because we shouldn't depend on tokio-core with emscripten
#[derive(Debug, Clone)]
pub struct CommonTransport {
    // The actual implementation of everything.
    inner: websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>>,
}

impl CommonTransport {
    /// Initializes the `CommonTransport`.
    #[inline]
    pub fn new(tokio_handle: tokio_core::reactor::Handle) -> CommonTransport {
        let tcp = tcp::TcpConfig::new(tokio_handle);
        let with_dns = dns::DnsConfig::new(tcp);
        let with_ws = websocket::WsConfig::new(with_dns);
        CommonTransport { inner: with_ws }
    }
}

impl Transport for CommonTransport {
    type Output = <websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>> as Transport>::Output;
    type Listener = <websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>> as Transport>::Listener;
    type ListenerUpgrade = <websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>> as Transport>::ListenerUpgrade;
    type Dial = <websocket::WsConfig<dns::DnsConfig<tcp::TcpConfig>> as Transport>::Dial;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        match self.inner.listen_on(addr) {
            Ok(res) => Ok(res),
            Err((inner, addr)) => {
                let trans = CommonTransport { inner: inner };
                Err((trans, addr))
            }
        }
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        match self.inner.dial(addr) {
            Ok(res) => Ok(res),
            Err((inner, addr)) => {
                let trans = CommonTransport { inner: inner };
                Err((trans, addr))
            }
        }
    }

    #[inline]
    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}
