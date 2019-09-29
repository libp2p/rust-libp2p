// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Transports, upgrades, multiplexing and node handling of *libp2p*.
//!
//! The main concepts of libp2p-core are:
//!
//! - A [`PeerId`] is a unique global identifier for a node on the network.
//!   Each node must have a different `PeerId`. Normally, a `PeerId` is the
//!   hash of the public key used to negotiate encryption on the
//!   communication channel, thereby guaranteeing that they cannot be spoofed.
//! - The [`Transport`] trait defines how to reach a remote node or listen for
//!   incoming remote connections. See the `transport` module.
//! - The [`StreamMuxer`] trait is implemented on structs that hold a connection
//!   to a remote and can subdivide this connection into multiple substreams.
//!   See the `muxing` module.
//! - The [`UpgradeInfo`], [`InboundUpgrade`] and [`OutboundUpgrade`] traits
//!   define how to upgrade each individual substream to use a protocol.
//!   See the `upgrade` module.

/// Multi-address re-export.
pub use multiaddr;
pub use multistream_select::Negotiated;

mod keys_proto;
mod peer_id;
mod translation;

#[cfg(test)]
mod tests;

pub mod either;
pub mod identity;
pub mod muxing;
pub mod nodes;
pub mod transport;
pub mod upgrade;

pub use multiaddr::Multiaddr;
pub use muxing::StreamMuxer;
pub use peer_id::PeerId;
pub use identity::PublicKey;
pub use transport::Transport;
pub use translation::address_translation;
pub use upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, UpgradeError, ProtocolName};
pub use nodes::ConnectionInfo;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// The socket comes from a dialer.
    Dialer,
    /// The socket comes from a listener.
    Listener,
}

impl std::ops::Not for Endpoint {
    type Output = Endpoint;

    fn not(self) -> Self::Output {
        match self {
            Endpoint::Dialer => Endpoint::Listener,
            Endpoint::Listener => Endpoint::Dialer
        }
    }
}

impl Endpoint {
    /// Is this endpoint a dialer?
    pub fn is_dialer(self) -> bool {
        if let Endpoint::Dialer = self {
            true
        } else {
            false
        }
    }

    /// Is this endpoint a listener?
    pub fn is_listener(self) -> bool {
        if let Endpoint::Listener = self {
            true
        } else {
            false
        }
    }
}

/// How we connected to a node.
#[derive(Debug, Clone)]
pub enum ConnectedPoint {
    /// We dialed the node.
    Dialer {
        /// Multiaddress that was successfully dialed.
        address: Multiaddr,
    },
    /// We received the node.
    Listener {
        /// Local connection address.
        local_addr: Multiaddr,
        /// Stack of protocols used to send back data to the remote.
        send_back_addr: Multiaddr,
    }
}

impl From<&'_ ConnectedPoint> for Endpoint {
    fn from(endpoint: &'_ ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl From<ConnectedPoint> for Endpoint {
    fn from(endpoint: ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl ConnectedPoint {
    /// Turns the `ConnectedPoint` into the corresponding `Endpoint`.
    pub fn to_endpoint(&self) -> Endpoint {
        match self {
            ConnectedPoint::Dialer { .. } => Endpoint::Dialer,
            ConnectedPoint::Listener { .. } => Endpoint::Listener
        }
    }

    /// Returns true if we are `Dialer`.
    pub fn is_dialer(&self) -> bool {
        match self {
            ConnectedPoint::Dialer { .. } => true,
            ConnectedPoint::Listener { .. } => false
        }
    }

    /// Returns true if we are `Listener`.
    pub fn is_listener(&self) -> bool {
        match self {
            ConnectedPoint::Dialer { .. } => false,
            ConnectedPoint::Listener { .. } => true
        }
    }
}

