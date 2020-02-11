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
//! are distinguished by their role as a _dialer_ (or _initiator_) or as a _listener_
//! (or _responder_). Thereby the dialer plays the active part, driving the protocol,
//! whereas the listener reacts to the messages received.
//!
//! The dialer has two options: it can either pick a protocol from the complete list
//! of protocols that the listener supports, or it can directly suggest a protocol.
//! Either way, a selected protocol is sent to the listener who can either accept (by
//! echoing the same protocol) or reject (by responding with a message stating
//! "not available"). If a suggested protocol is not available, the dialer may
//! suggest another protocol. This process continues until a protocol is agreed upon,
//! yielding a [`Negotiated`](self::Negotiated) stream, or the dialer has run out of
//! alternatives.
//!
//! See [`dialer_select_proto`](self::dialer_select_proto) and
//! [`listener_select_proto`](self::listener_select_proto).
//!
//! ## [`Negotiated`](self::Negotiated)
//!
//! When a dialer or listener participating in a negotiation settles
//! on a protocol to use, the [`DialerSelectFuture`] respectively
//! [`ListenerSelectFuture`] yields a [`Negotiated`](self::Negotiated)
//! I/O stream.
//!
//! Notably, when a `DialerSelectFuture` resolves to a `Negotiated`, it may not yet
//! have written the last negotiation message to the underlying I/O stream and may
//! still be expecting confirmation for that protocol, despite having settled on
//! a protocol to use.
//!
//! Similarly, when a `ListenerSelectFuture` resolves to a `Negotiated`, it may not
//! yet have sent the last negotiation message despite having settled on a protocol
//! proposed by the dialer that it supports.
//!
//! This behaviour allows both the dialer and the listener to send data
//! relating to the negotiated protocol together with the last negotiation
//! message(s), which, in the case of the dialer only supporting a single
//! protocol, results in 0-RTT negotiation. Note, however, that a dialer
//! that performs multiple 0-RTT negotiations in sequence for different
//! protocols layered on top of each other may trigger undesirable behaviour
//! for a listener not supporting one of the intermediate protocols.
//! See [`dialer_select_proto`](self::dialer_select_proto).
//!
//! ## Examples
//!
//! For a dialer:
//!
//! ```no_run
//! # fn main() {
//! use bytes::Bytes;
//! use multistream_select::{dialer_select_proto, Version};
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
//!         dialer_select_proto(io, protos, Version::V1)
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

pub use self::negotiated::{Negotiated, NegotiatedComplete, NegotiationError};
pub use self::protocol::{ProtocolError, Version};
pub use self::dialer_select::{dialer_select_proto, DialerSelectFuture};
pub use self::listener_select::{listener_select_proto, ListenerSelectFuture};

