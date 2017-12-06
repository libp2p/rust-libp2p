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

use futures::future::Future;
use std::io::Error as IoError;
use tokio_io::{AsyncRead, AsyncWrite};

/// Implemented on objects that can be turned into a substream.
///
/// > **Note**: The methods of this trait consume the object, but if the object implements `Clone`
/// >           then you can clone it and keep the original in order to open additional substreams.
pub trait StreamMuxer {
	/// Type of the object that represents the raw substream where data can be read and written.
	type Substream: AsyncRead + AsyncWrite;
	/// Future that will be resolved when a new incoming substream is open.
	type InboundSubstream: Future<Item = Self::Substream, Error = IoError>;
	/// Future that will be resolved when the outgoing substream is open.
	type OutboundSubstream: Future<Item = Self::Substream, Error = IoError>;

	/// Produces a future that will be resolved when a new incoming substream arrives.
	fn inbound(self) -> Self::InboundSubstream;

	/// Opens a new outgoing substream, and produces a future that will be resolved when it becomes
	/// available.
	fn outbound(self) -> Self::OutboundSubstream;
}
