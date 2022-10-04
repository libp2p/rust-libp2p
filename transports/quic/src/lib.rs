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

//! Implementation of the libp2p `Transport` and `StreamMuxer` traits for QUIC.
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
//! use libp2p_quic::{AsyncStdTransport, Config};
//! use libp2p_core::{Multiaddr, Transport};
//!
//! let keypair = libp2p_core::identity::Keypair::generate_ed25519();
//! let quic_config = Config::new(&keypair).expect("could not make config");
//!
//! let mut quic_transport = AsyncStdTransport::new(quic_config);
//!
//! let addr = "/ip4/127.0.0.1/udp/12345/quic".parse().expect("bad address?");
//! quic_transport.listen_on(addr).expect("listen error.");
//! #
//! # Ok(())
//! # }
//! ```
//!
//! The [`QuicTransport`] struct implements the [`libp2p_core::Transport`]. See the
//! documentation of [`libp2p_core`] and of libp2p in general to learn how to use the
//! [`Transport`][libp2p_core::Transport] trait.
//!
//! Note that QUIC provides transport, security, and multiplexing in a single protocol.  Therefore,
//! QUIC connections do not need to be upgraded. You will get a compile-time error if you try.
//! Instead, you must pass all needed configuration into the constructor.
//!

#![deny(unsafe_code)]

mod connection;
mod endpoint;
mod muxer;
mod tls;
mod transport;
mod upgrade;

pub use connection::ConnectionError;
pub use endpoint::Config;
pub use muxer::QuicMuxer;
pub use quinn_proto::ConnectError as DialError;
#[cfg(feature = "async-std")]
pub use transport::{AsyncStd, AsyncStdTransport};
pub use transport::{Provider, QuicTransport, TransportError};
#[cfg(feature = "tokio")]
pub use transport::{Tokio, TokioTransport};
pub use upgrade::Upgrade;
