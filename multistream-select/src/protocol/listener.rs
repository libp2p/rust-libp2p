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

//! Contains the `Listener` wrapper, which allows raw communications with a dialer.

use bytes::{Bytes, BytesMut};
use futures::{Async, AsyncSink, Future, Poll, Sink, StartSend, Stream};
use length_delimited::LengthDelimitedFramedRead;
use protocol::DialerToListenerMessage;
use protocol::ListenerToDialerMessage;
use protocol::MULTISTREAM_PROTOCOL_WITH_LF;
use protocol::MultistreamSelectError;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited::Builder as LengthDelimitedBuilder;
use tokio_io::codec::length_delimited::FramedWrite as LengthDelimitedFramedWrite;
use varint;

/// Wraps around a `AsyncRead+AsyncWrite`. Assumes that we're on the listener's side. Produces and
/// accepts messages.
pub struct Listener<R> {
	inner: LengthDelimitedFramedRead<Bytes, LengthDelimitedFramedWrite<R, BytesMut>>,
}

impl<R> Listener<R>
    where R: AsyncRead + AsyncWrite
{
	/// Takes ownership of a socket and starts the handshake. If the handshake succeeds, the
	/// future returns a `Listener`.
	pub fn new<'a>(inner: R) -> Box<Future<Item = Listener<R>, Error = MultistreamSelectError> + 'a>
		where R: 'a
	{
		let write = LengthDelimitedBuilder::new().length_field_length(1).new_write(inner);
		let inner = LengthDelimitedFramedRead::<Bytes, _>::new(write);

		let future = inner.into_future()
		                  .map_err(|(e, _)| e.into())
		                  .and_then(|(msg, rest)| {
			if msg.as_ref().map(|b| &b[..]) != Some(MULTISTREAM_PROTOCOL_WITH_LF) {
				return Err(MultistreamSelectError::FailedHandshake);
			}
			Ok(rest)
		})
		                  .and_then(|socket| {
			socket.send(BytesMut::from(MULTISTREAM_PROTOCOL_WITH_LF)).from_err()
		})
		                  .map(|inner| Listener { inner: inner });

		Box::new(future)
	}

	/// Grants back the socket. Typically used after a `ProtocolRequest` has been received and a
	/// `ProtocolAck` has been sent back.
	#[inline]
	pub fn into_inner(self) -> R {
		self.inner.into_inner().into_inner()
	}
}

impl<R> Sink for Listener<R>
    where R: AsyncRead + AsyncWrite
{
	type SinkItem = ListenerToDialerMessage;
	type SinkError = MultistreamSelectError;

	#[inline]
	fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
		match item {
			ListenerToDialerMessage::ProtocolAck { name } => {
				if !name.starts_with(b"/") {
					return Err(MultistreamSelectError::WrongProtocolName);
				}
				let mut protocol = BytesMut::from(name);
				protocol.extend_from_slice(&[b'\n']);
				match self.inner.start_send(protocol) {
					Ok(AsyncSink::Ready) => Ok(AsyncSink::Ready),
					Ok(AsyncSink::NotReady(mut protocol)) => {
						let protocol_len = protocol.len();
						protocol.truncate(protocol_len - 1);
						let protocol = protocol.freeze();
						Ok(
							AsyncSink::NotReady(ListenerToDialerMessage::ProtocolAck { name: protocol }),
						)
					}
					Err(err) => Err(err.into()),
				}
			}

			ListenerToDialerMessage::NotAvailable => {
				match self.inner.start_send(BytesMut::from(&b"na\n"[..])) {
					Ok(AsyncSink::Ready) => Ok(AsyncSink::Ready),
					Ok(AsyncSink::NotReady(_)) => {
						Ok(AsyncSink::NotReady(ListenerToDialerMessage::NotAvailable))
					}
					Err(err) => Err(err.into()),
				}
			}

			ListenerToDialerMessage::ProtocolsListResponse { list } => {
				use std::iter;

				let mut out_msg = varint::encode(list.len());
				for elem in list.iter() {
					out_msg.extend(iter::once(b'\r'));
					out_msg.extend_from_slice(elem);
					out_msg.extend(iter::once(b'\n'));
				}

				match self.inner.start_send(BytesMut::from(out_msg)) {
					Ok(AsyncSink::Ready) => Ok(AsyncSink::Ready),
					Ok(AsyncSink::NotReady(_)) => {
						let m = ListenerToDialerMessage::ProtocolsListResponse { list };
						Ok(AsyncSink::NotReady(m))
					}
					Err(err) => Err(err.into()),
				}
			}
		}
	}

	#[inline]
	fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
		Ok(self.inner.poll_complete()?)
	}
}

impl<R> Stream for Listener<R>
    where R: AsyncRead + AsyncWrite
{
	type Item = DialerToListenerMessage;
	type Error = MultistreamSelectError;

	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		loop {
			let mut frame = match self.inner.poll() {
				Ok(Async::Ready(Some(frame))) => frame,
				Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
				Ok(Async::NotReady) => return Ok(Async::NotReady),
				Err(err) => return Err(err.into()),
			};

			if frame.get(0) == Some(&b'/') && frame.last() == Some(&b'\n') {
				let frame_len = frame.len();
				let protocol = frame.split_to(frame_len - 1);
				return Ok(Async::Ready(
					Some(DialerToListenerMessage::ProtocolRequest { name: protocol }),
				));

			} else if frame == &b"ls\n"[..] {
				return Ok(Async::Ready(Some(DialerToListenerMessage::ProtocolsListRequest)));

			} else {
				return Err(MultistreamSelectError::UnknownMessage);
			}
		}
	}
}

#[cfg(test)]
mod tests {
	extern crate tokio_core;
	use bytes::Bytes;
	use futures::{Sink, Stream};
	use futures::Future;
	use protocol::{Dialer, Listener, ListenerToDialerMessage, MultistreamSelectError};
	use self::tokio_core::net::{TcpListener, TcpStream};
	use self::tokio_core::reactor::Core;

	#[test]
	fn wrong_proto_name() {
		let mut core = Core::new().unwrap();

		let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
		let listener_addr = listener.local_addr().unwrap();

		let server = listener.incoming()
		                     .into_future()
		                     .map_err(|(e, _)| e.into())
		                     .and_then(move |(connec, _)| Listener::new(connec.unwrap().0))
		                     .and_then(|listener| {
			let proto_name = Bytes::from("invalid-proto");
			listener.send(ListenerToDialerMessage::ProtocolAck { name: proto_name })
		});

		let client = TcpStream::connect(&listener_addr, &core.handle())
			.from_err()
			.and_then(move |stream| Dialer::new(stream));

		match core.run(server.join(client)) {
			Err(MultistreamSelectError::WrongProtocolName) => (),
			_ => panic!(),
		}
	}
}
