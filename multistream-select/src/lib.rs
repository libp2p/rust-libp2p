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

//! # Multistream-select
//!
//! This crate implements the `multistream-select` protocol, which is the protocol used by libp2p
//! to negotiate which protocol to use with the remote.
//!
//! > **Note**: This crate is used by the internals of *libp2p*, and it is not required to
//! > understand it in order to use *libp2p*.
//!
//! Whenever a new connection or a new multiplexed substream is opened, libp2p uses
//! `multistream-select` to negotiate with the remote which protocol to use. After a protocol has
//! been successfully negotiated, the stream (ie. the connection or the multiplexed substream)
//! immediately stops using `multistream-select` and starts using the negotiated protocol.
//!
//! ## Protocol explanation
//!
//! The dialer has two options available: either request the list of protocols that the listener
//! supports, or suggest a protocol. If a protocol is suggested, the listener can either accept (by
//! answering with the same protocol name) or refuse the choice (by answering "not available").
//!
//! ## Examples
//!
//! For a dialer:
//!
//! ```no_run
//! extern crate bytes;
//! extern crate futures;
//! extern crate multistream_select;
//! extern crate tokio_current_thread;
//! extern crate tokio_tcp;
//!
//! # fn main() {
//! use bytes::Bytes;
//! use multistream_select::dialer_select_proto;
//! use futures::{Future, Sink, Stream};
//! use tokio_tcp::TcpStream;
//!
//! #[derive(Debug, Copy, Clone)]
//! enum MyProto { Echo, Hello }
//!
//! let client = TcpStream::connect(&"127.0.0.1:10333".parse().unwrap())
//!     .from_err()
//!     .and_then(move |connec| {
//!         let protos = vec![
//!             (Bytes::from("/echo/1.0.0"), <Bytes as PartialEq>::eq, MyProto::Echo),
//!             (Bytes::from("/hello/2.5.0"), <Bytes as PartialEq>::eq, MyProto::Hello),
//!         ]
//!                         .into_iter();
//!         dialer_select_proto(connec, protos).map(|r| r.0)
//!     });
//!
//! let negotiated_protocol: MyProto = tokio_current_thread::block_on_all(client)
//!     .expect("failed to find a protocol");
//! println!("negotiated: {:?}", negotiated_protocol);
//! # }
//! ```
//!
//! For a listener:
//!
//! ```no_run
//! extern crate bytes;
//! extern crate futures;
//! extern crate multistream_select;
//! extern crate tokio_current_thread;
//! extern crate tokio_tcp;
//!
//! # fn main() {
//! use bytes::Bytes;
//! use multistream_select::listener_select_proto;
//! use futures::{Future, Sink, Stream};
//! use tokio_tcp::TcpListener;
//!
//! #[derive(Debug, Copy, Clone)]
//! enum MyProto { Echo, Hello }
//!
//! let server = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap()
//!     .incoming()
//!     .from_err()
//!     .and_then(move |connec| {
//!         let protos = vec![
//!             (Bytes::from("/echo/1.0.0"), <Bytes as PartialEq>::eq, MyProto::Echo),
//!             (Bytes::from("/hello/2.5.0"), <Bytes as PartialEq>::eq, MyProto::Hello),
//!         ]
//!                         .into_iter();
//!         listener_select_proto(connec, protos)
//!     })
//!     .for_each(|(proto, _connec)| {
//!         println!("new remote with {:?} negotiated", proto);
//!         Ok(())
//!     });
//!
//! tokio_current_thread::block_on_all(server).expect("failed to run server");
//! # }
//! ```

extern crate bytes;
extern crate futures;
#[macro_use]
extern crate log;
extern crate smallvec;
extern crate tokio_io;
extern crate unsigned_varint;

mod dialer_select;
mod error;
mod length_delimited;
mod listener_select;
mod tests;

pub mod protocol;

pub use self::dialer_select::dialer_select_proto;
pub use self::error::ProtocolChoiceError;
pub use self::listener_select::listener_select_proto;
