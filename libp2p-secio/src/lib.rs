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

//! # Implementation of the `secio` protocol.
//!
//! The `secio` protocol is a middleware that will encrypt and decrypt communications going
//! through a socket (or anything that implements `AsyncRead + AsyncWrite`).
//!
//! You can add the `secio` layer over a socket by calling `SecioMiddleware::handshake()`. This
//! method will perform a handshake with the host, and return a future that corresponds to the
//! moment when the handshake succeeds or errored. On success, the future produces a
//! `SecioMiddleware` that implements `Sink` and `Stream` and can be used to send packets of data.

extern crate bytes;
extern crate crypto;
extern crate futures;
extern crate protobuf;
extern crate rand;
extern crate ring;
extern crate tokio_core;
extern crate tokio_io;
extern crate untrusted;

pub use self::error::SecioError;

use bytes::BytesMut;
use futures::{Future, Poll, StartSend, Sink, Stream};
use ring::signature::RSAKeyPair;
use std::io::Error as IoError;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};

mod algo_support;
mod codec;
mod error;
mod keys_proto;
mod handshake;
mod structs_proto;

/// Wraps around an object that implements `AsyncRead` and `AsyncWrite`.
///
/// Implements `Sink` and `Stream` whose items are frames of data. Each frame is encoded
/// individually, so you are encouraged to group data in few frames if possible.
pub struct SecioMiddleware<S> {
	inner: codec::FullCodec<S>,
	remote_pubkey_der: Vec<u8>,
}

impl<S> SecioMiddleware<S>
    where S: AsyncRead + AsyncWrite
{
	/// Attempts to perform a handshake on the given socket.
	///
	/// `local_public_key` and `local_private_key` must match. `local_public_key` must be in the
	/// DER format.
	///
	/// On success, produces a `SecioMiddleware` that can then be used to encode/decode
	/// communications.
	pub fn handshake<'a>(
		socket: S,
		local_public_key: Vec<u8>,
		local_private_key: Arc<RSAKeyPair>,
	) -> Box<Future<Item = SecioMiddleware<S>, Error = SecioError> + 'a>
		where S: 'a
	{
		let fut = handshake::handshake(socket, local_public_key, local_private_key)
			.map(|(inner, pubkey)| {
				SecioMiddleware {
					inner: inner,
					remote_pubkey_der: pubkey,
				}
			});
		Box::new(fut)
	}

	/// Returns the public key of the remote in the `DER` format.
	#[inline]
	pub fn remote_public_key_der(&self) -> &[u8] {
		&self.remote_pubkey_der
	}
}

impl<S> Sink for SecioMiddleware<S>
    where S: AsyncRead + AsyncWrite
{
	type SinkItem = BytesMut;
	type SinkError = IoError;

	#[inline]
	fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
		self.inner.start_send(item)
	}

	#[inline]
	fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
		self.inner.poll_complete()
	}
}

impl<S> Stream for SecioMiddleware<S>
    where S: AsyncRead + AsyncWrite
{
	type Item = Vec<u8>;
	type Error = SecioError;

	#[inline]
	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		self.inner.poll()
	}
}
