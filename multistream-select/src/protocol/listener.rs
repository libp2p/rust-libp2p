// Copyright 2017 Parity Technologies (UK) Ltd.

// Libp2p-rs is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Libp2p-rs is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Libp2p-rs.  If not, see <http://www.gnu.org/licenses/>.

//! Contains the `Listener` wrapper, which allows raw communications with a dialer.

use bytes::BytesMut;
use futures::{Async, AsyncSink, Future, Poll, Sink, StartSend, Stream};
use protocol::DialerToListenerMessage;
use protocol::ListenerToDialerMessage;

use protocol::MULTISTREAM_PROTOCOL_WITH_LF;
use protocol::MultistreamSelectError;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited::Builder as LengthDelimitedBuilder;
use tokio_io::codec::length_delimited::Framed as LengthDelimitedFramed;

/// Wraps around a `AsyncRead+AsyncWrite`. Assumes that we're on the listener's side. Produces and
/// accepts messages.
pub struct Listener<R> {
	inner: LengthDelimitedFramed<R, BytesMut>,
}

impl<R> Listener<R>
    where R: AsyncRead + AsyncWrite
{
	/// Takes ownership of a socket and starts the handshake. If the handshake succeeds, the
	/// future returns a `Listener`.
	pub fn new<'a>(inner: R) -> Box<Future<Item = Listener<R>, Error = MultistreamSelectError> + 'a>
		where R: 'a
	{
		// TODO: use Jack's lib instead
		let inner = LengthDelimitedBuilder::new().length_field_length(1).new_framed(inner);

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
		self.inner.into_inner()
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

			ListenerToDialerMessage::ProtocolsListResponse { list: _list } => {
				unimplemented!()
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
			let frame = match self.inner.poll() {
				Ok(Async::Ready(Some(frame))) => frame,
				Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
				Ok(Async::NotReady) => return Ok(Async::NotReady),
				Err(err) => return Err(err.into()),
			};

			if frame.get(0) == Some(&b'/') && frame.last() == Some(&b'\n') {
				let frame = frame.freeze();
				let protocol = frame.slice_to(frame.len() - 1);
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
	use bytes::Bytes;
	use futures::{Sink, Stream};
	use futures::Future;
	use protocol::{Dialer, Listener, ListenerToDialerMessage, MultistreamSelectError};
	use tokio_core::net::{TcpListener, TcpStream};
	use tokio_core::reactor::Core;

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
