// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! # Multistream-select Protocol Negotiation
//!
//! This crate implements the `multistream-select` protocol, which is the protocol
//! used by libp2p to negotiate which application-layer protocol to use with the
//! remote on a connection or substream.
//!
//! > **Note**: This crate is used primarily by core components of *libp2p* and it
//! > is usually not used directly on its own.
//!
//! ## Roles
//!
//! Two peers using the multistream-select negotiation protocol on an I/O stream
//! are distinguished by their role as a _dialer_ or a _listener_. Thereby the dialer
//! (or _initiator_) plays the active part, driving the protocol, whereas the listener
//! (or _responder_) reacts to the messages received.
//!
//! The dialer has two options: it can either pick a protocol from the complete list
//! of protocols that the listener supports, or it can directly suggest a protocol.
//! Either way, a selected protocol is sent to the listener who can either accept (by
//! echoing the same protocol) or reject (by responding with a message stating
//! "not available"). If a suggested protocol is not available, the dialer may
//! suggest another protocol. This process continues until a protocol is agreed upon,
//! yielding a [`Negotiated`](self::Negotiated) stream, or the dialer has run out of alternatives.
//!
//! See [`dialer_select_proto`](self::dialer_select_proto) and
//! [`listener_select_proto`](self::listener_select_proto).
//!
//! ## Examples
//!
//! For a dialer:
//!
//! ```no_run
//! # fn main() {
//! use bytes::Bytes;
//! use multistream_select::dialer_select_proto;
//! use futures::{Future, Sink, Stream};
//! use tokio_tcp::TcpStream;
//! use tokio::runtime::current_thread::Runtime;
//!
//! #[derive(Debug, Copy, Clone)]
//! enum MyProto { Echo, Hello }
//!
//! let client = TcpStream::connect(&"127.0.0.1:10333".parse().unwrap())
//!     .from_err()
//!     .and_then(move |io| {
//!         let protos = vec![b"/echo/1.0.0", b"/echo/2.5.0"];
//!         dialer_select_proto(io, protos) // .map(|r| r.0)
//!     })
//!     .map(|(protocol, _io)| protocol);
//!
//! let mut rt = Runtime::new().unwrap();
//! let protocol = rt.block_on(client).expect("failed to find a protocol");
//! println!("Negotiated protocol: {:?}", protocol);
//! # }
//! ```
//!

mod dialer_select;
mod length_delimited;
mod listener_select;
mod negotiated;
mod protocol;
mod tests;

pub use self::negotiated::{Negotiated, NegotiationError};
pub use self::protocol::ProtocolError;
pub use self::dialer_select::{dialer_select_proto, DialerSelectFuture};
pub use self::listener_select::{listener_select_proto, ListenerSelectFuture};

