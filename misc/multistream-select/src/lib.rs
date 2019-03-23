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
//! been successfully negotiated, the stream (i.e. the connection or the multiplexed substream)
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
//!     .and_then(move |connec| {
//!         let protos = vec![b"/echo/1.0.0", b"/echo/2.5.0"];
//!         dialer_select_proto(connec, protos).map(|r| r.0)
//!     });
//!
//! let mut rt = Runtime::new().unwrap();
//! let negotiated_protocol = rt.block_on(client).expect("failed to find a protocol");
//! println!("negotiated: {:?}", negotiated_protocol);
//! # }
//! ```
//!

mod dialer_select;
mod error;
mod length_delimited;
mod listener_select;
mod tests;

mod protocol;

use futures::prelude::*;
use std::io;

pub use self::dialer_select::{dialer_select_proto, DialerSelectFuture};
pub use self::error::ProtocolChoiceError;
pub use self::listener_select::{listener_select_proto, ListenerSelectFuture};

/// A stream after it has been negotiated.
pub struct Negotiated<TInner>(pub(crate) TInner);

impl<TInner> io::Read for Negotiated<TInner>
where
    TInner: io::Read
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<TInner> tokio_io::AsyncRead for Negotiated<TInner>
where
    TInner: tokio_io::AsyncRead
{
}

impl<TInner> io::Write for Negotiated<TInner>
where
    TInner: io::Write
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<TInner> tokio_io::AsyncWrite for Negotiated<TInner>
where
    TInner: tokio_io::AsyncWrite
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.0.shutdown()
    }
}
