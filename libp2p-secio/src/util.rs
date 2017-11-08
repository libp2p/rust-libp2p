use futures::Async;
use futures::Future;
use futures::Poll;
use protobuf::Message as ProtobufMessage;
use protobuf::MessageStatic as ProtobufMessageStatic;
use protobuf::core::parse_from_bytes;
use std::io::Cursor;
use std::marker::PhantomData;
use std::mem;
use tokio_io::AsyncRead;

use error::SecioError;

/// Reads a single protobuf message from an `AsyncRead`.
///
/// Returns a future that produces the message, its raw bytes representation, and the remaining
/// `AsyncRead`.
pub fn read_protobuf_message<'a, M, R>(
	read: R,
) -> Box<Future<Item = (M, Vec<u8>, R), Error = SecioError> + 'a>
where
	R: AsyncRead + 'a,
	M: ProtobufMessage + ProtobufMessageStatic,
{
	struct Fut<M, R> {
		marker: PhantomData<M>,
		read: Option<R>,
		local_buf: Vec<u8>,
	}
	impl<M, R> Future for Fut<M, R>
	where
		R: AsyncRead,
		M: ProtobufMessage + ProtobufMessageStatic,
	{
		type Item = (M, Vec<u8>, R);
		type Error = SecioError;
		#[inline]
		fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
			loop {
				let local_buf_len = self.local_buf.len();

				let inner_poll = {
					let r = match self.read {
						Some(ref mut r) => r,
						None => panic!("the protobuf reader can only produce one message"),
					};
					r.read_buf(&mut Cursor::new(&mut self.local_buf[local_buf_len - 1..]))
				};

				// Add a limit to the maximum buffer size of the handshake.
				if self.local_buf.len() >= 32768 {
					return Err(SecioError::HandshakeParsingFailure);
				}

				match inner_poll {
					Ok(Async::Ready(0)) => {
						// eof
						return Err(SecioError::HandshakeParsingFailure);
					}
					Ok(Async::Ready(n)) => {
						debug_assert_eq!(n, 1);
						if let Ok(message) = parse_from_bytes(&self.local_buf) {
							let raw_buf = mem::replace(&mut self.local_buf, Vec::new());
							return Ok(Async::Ready((message, raw_buf, self.read.take().unwrap())));
						} else {
							self.local_buf.push(0);
						}
					}
					Ok(Async::NotReady) => return Ok(Async::NotReady),
					Err(err) => return Err(err.into()),
				};
			}
		}
	}
	Box::new(Fut {
		marker: PhantomData,
		read: Some(read),
		local_buf: {
			let mut v = Vec::with_capacity(mem::size_of::<M>()); // Note: this is an estimate
			v.push(0);
			v
		},
	})
}
