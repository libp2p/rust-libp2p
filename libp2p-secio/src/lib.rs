extern crate bytes;
extern crate crypto;
extern crate futures;
extern crate libp2p_swarm;
extern crate protobuf;
extern crate rand;
extern crate ring;
extern crate rw_stream_sink;
extern crate tokio_core;
extern crate tokio_io;
extern crate untrusted;

pub use self::error::SecioError;

use bytes::BytesMut;
use futures::{Future, Poll, StartSend, Sink, Stream};
use futures::stream::MapErr as StreamMapErr;
use ring::signature::Ed25519KeyPair;
use rw_stream_sink::RwStreamSink;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};

mod algo_support;
mod codec;
mod error;
mod handshake;
mod structs_proto;
mod util;

pub struct SecioConnUpgrade {
	local_public_key: Vec<u8>,
	local_private_key: Arc<Ed25519KeyPair>,
}

impl libp2p_swarm::ConnectionUpgrade<Box<libp2p_swarm::AsyncReadWrite>> for SecioConnUpgrade {
    type Output = RwStreamSink<StreamMapErr<SecIoMiddleware<Box<libp2p_swarm::AsyncReadWrite>>, fn(SecioError)->IoError>>;
	type Future = Box<Future<Item = Self::Output, Error = IoError>>;

	#[inline]
    fn upgrade(&self, incoming: Box<libp2p_swarm::AsyncReadWrite>) -> Self::Future {
		let fut = SecIoMiddleware::handshake(incoming, self.local_public_key.clone(),
										   		     self.local_private_key.clone());
		let wrapped = fut.map(|stream_sink| {
			let mapped = stream_sink.map_err(map_err as fn(_)->_);
			RwStreamSink::new(mapped)
		}).map_err(map_err);
		Box::new(wrapped)
	}
}

#[inline]
fn map_err(err: SecioError) -> IoError {
	IoError::new(IoErrorKind::InvalidData, err)
}

/// Wraps around an object that implements `AsyncRead` and `AsyncWrite`.
pub struct SecIoMiddleware<S> {
	inner: codec::FullCodec<S>,
	remote_pubkey: Vec<u8>,
}

impl<S> SecIoMiddleware<S>
where
	S: AsyncRead + AsyncWrite,
{
	/// Attempts to perform a handshake on the given socket.
	///
	/// On success, produces a `SecIoMiddleware` that can then be used to encode/decode
	/// communications.
	pub fn handshake<'a>(
		socket: S,
		local_public_key: Vec<u8>,
		local_private_key: Arc<Ed25519KeyPair>,
	) -> Box<Future<Item = SecIoMiddleware<S>, Error = SecioError> + 'a>
	where
		S: 'a,
	{
		let fut = handshake::handshake(socket, local_public_key, local_private_key)
			.map(|(inner, pubkey)| SecIoMiddleware { inner: inner, remote_pubkey: pubkey });
		Box::new(fut)
	}

	/// Returns the public key of the remote.
	#[inline]
	pub fn remote_public_key(&self) -> &[u8] {
		&self.remote_pubkey
	}
}

impl<S> Sink for SecIoMiddleware<S>
where
	S: AsyncRead + AsyncWrite,
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

impl<S> Stream for SecIoMiddleware<S>
where
	S: AsyncRead + AsyncWrite,
{
	type Item = Vec<u8>;
	type Error = SecioError;

	#[inline]
	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		self.inner.poll()
	}
}
