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

//! Transport, protocol upgrade and swarm systems of *libp2p*.
//!
//! This crate contains all the core traits and mechanisms of the transport and swarm systems
//! of *libp2p*.
//!
//! # Overview
//!
//! This documentation focuses on the concepts of *libp2p-core*, and is interesting mostly if you
//! want to extend *libp2p* with new protocols. If you only want to use libp2p, you might find the
//! documentation of the main *libp2p* crate more interesting.
//!
//! The main concepts of libp2p are:
//!
//! - A `PeerId` is a unique global identifier for a node on the network. Each node must have a
//!   different `PeerId`. Normally, a `PeerId` is the hash of the public key used to negotiate
//!   encryption on the communication channel, thereby guaranteeing that they cannot be spoofed.
//! - The `Transport` trait defines how to reach a remote node or listen for incoming remote
//!   connections. See the `transport` module.
//! - The `Swarm` struct contains all active and pending connections to remotes and manages the
//!   state of all the substreams that have been opened, and all the upgrades that were built upon
//!   these substreams.
//! - Use the `NetworkBehaviour` trait to customize the behaviour of a `Swarm`. It is the
//!   `NetworkBehaviour` that controls what happens on the network. Multiple types that implement
//!   `NetworkBehaviour` can be composed into a single behaviour.
//! - The `Topology` trait is implemented for types that hold the layout of a network. When other
//!   components need the network layout to operate, they are passed an instance of a `Topology`.
//! - The `StreamMuxer` trait is implemented on structs that hold a connection to a remote and can
//!   subdivide this connection into multiple substreams. See the `muxing` module.
//! - The `UpgradeInfo`, `InboundUpgrade` and `OutboundUpgrade` traits define how to upgrade each
//!   individual substream to use a protocol. See the `upgrade` module.
//! - The `ProtocolsHandler` trait defines how each active connection to a remote should behave:
//!   how to handle incoming substreams, which protocols are supported, when to open a new
//!   outbound substream, etc. See the `protocols_handler` trait.
//!
//! # High-level APIs vs low-level APIs
//!
//! This crate provides two sets of APIs:
//!
//! - The low-level APIs are contained within the `nodes` module. See the documentation for more
//!   information.
//! - The high-level APIs include the concepts of `Swarm`, `ProtocolsHandler`, `NetworkBehaviour`
//!   and `Topology`.


/// Multi-address re-export.
pub use multiaddr;

mod keys_proto;
mod peer_id;

#[cfg(test)]
mod tests;

pub mod either;
pub mod identity;
pub mod muxing;
pub mod nodes;
pub mod protocols_handler;
pub mod swarm;
pub mod transport;
pub mod upgrade;

pub use self::multiaddr::Multiaddr;
pub use self::muxing::StreamMuxer;
pub use self::peer_id::PeerId;
pub use self::protocols_handler::{ProtocolsHandler, ProtocolsHandlerEvent};
pub use self::identity::PublicKey;
pub use self::swarm::Swarm;
pub use self::transport::Transport;
pub use self::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, UpgradeError, ProtocolName};

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

