// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! Implementation of the QUIC transport protocol for libp2p.
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! # #[cfg(not(feature = "async-std"))]
//! # fn main() {}
//! #
//! # #[cfg(feature = "async-std")]
//! # fn main() -> std::io::Result<()> {
//! #
//! use libp2p_quic as quic;
//! use libp2p_core::{Multiaddr, Transport, transport::ListenerId};
//!
//! let keypair = libp2p_identity::Keypair::generate_ed25519();
//! let quic_config = quic::Config::new(&keypair);
//!
//! let mut quic_transport = quic::async_std::Transport::new(quic_config);
//!
//! let addr = "/ip4/127.0.0.1/udp/12345/quic-v1".parse().expect("address should be valid");
//! quic_transport.listen_on(ListenerId::next(), addr).expect("listen error.");
//! #
//! # Ok(())
//! # }
//! ```
//!
//! The [`GenTransport`] struct implements the [`libp2p_core::Transport`]. See the
//! documentation of [`libp2p_core`] and of libp2p in general to learn how to use the
//! [`Transport`][libp2p_core::Transport] trait.
//!
//! Note that QUIC provides transport, security, and multiplexing in a single protocol.  Therefore,
//! QUIC connections do not need to be upgraded. You will get a compile-time error if you try.
//! Instead, you must pass all needed configuration into the constructor.
//!

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod config;
mod connection;
mod hole_punching;
mod provider;
mod transport;

use std::net::SocketAddr;

pub use config::Config;
pub use connection::{Connecting, Connection, Stream};

#[cfg(feature = "async-std")]
pub use provider::async_std;
#[cfg(feature = "tokio")]
pub use provider::tokio;
pub use provider::Provider;
pub use transport::GenTransport;

/// Errors that may happen on the [`GenTransport`] or a single [`Connection`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error while trying to reach a remote.
    #[error(transparent)]
    Reach(#[from] ConnectError),

    /// Error after the remote has been reached.
    #[error(transparent)]
    Connection(#[from] ConnectionError),

    /// I/O Error on a socket.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The task to drive a quic endpoint has crashed.
    #[error("Endpoint driver crashed")]
    EndpointDriverCrashed,

    /// The [`Connecting`] future timed out.
    #[error("Handshake with the remote timed out.")]
    HandshakeTimedOut,

    /// Error when `Transport::dial_as_listener` is called without an active listener.
    #[error("Tried to dial as listener without an active listener.")]
    NoActiveListenerForDialAsListener,

    /// Error when holepunching for a remote is already in progress
    #[error("Already punching hole for {0}).")]
    HolePunchInProgress(SocketAddr),
}

/// Dialing a remote peer failed.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ConnectError(quinn::ConnectError);

/// Error on an established [`Connection`].
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ConnectionError(quinn::ConnectionError);
