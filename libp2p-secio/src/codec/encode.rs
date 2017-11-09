use bytes::BytesMut;
use bytes::buf::BufMut;
use crypto::symmetriccipher::SynchronousStreamCipher;
use futures::Poll;
use futures::StartSend;
use futures::sink::Sink;
use futures::stream::Stream;
use ring::hmac;

/// Wraps around a `Sink`. Encodes the buffers passed to it and passes it to the underlying sink.
///
/// This struct implements `Sink`. It expects individual frames of data, and outputs individual
/// frames as well, most notably without the length prefix. The mechanism for adding the length
/// prefix is not covered by this module.
///
/// Also implements `Stream` for convenience.
pub struct EncoderMiddleware<S> {
	cipher_state: Box<SynchronousStreamCipher>,
	hmac_key: hmac::SigningKey,
	raw_sink: S,
}

impl<S> EncoderMiddleware<S> {
	pub fn new(
		raw_sink: S,
		cipher: Box<SynchronousStreamCipher>,
		hmac_key: hmac::SigningKey,
	) -> EncoderMiddleware<S> {
		EncoderMiddleware {
			cipher_state: cipher,
			hmac_key: hmac_key,
			raw_sink: raw_sink,
		}
	}
}

impl<S> Sink for EncoderMiddleware<S>
where
	S: Sink<SinkItem = BytesMut>,
{
	type SinkItem = BytesMut;
	type SinkError = S::SinkError;

	fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
		let mut out_buffer = vec![0; item.len()];
		self.cipher_state.process(&item, &mut out_buffer);

		let mut out_buffer = BytesMut::from(out_buffer);

		let signature = hmac::sign(&self.hmac_key, &out_buffer);
		out_buffer.reserve(signature.as_ref().len());
		out_buffer.put_slice(signature.as_ref());

		self.raw_sink.start_send(out_buffer)
	}

	#[inline]
	fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
		self.raw_sink.poll_complete()
	}
}

impl<S> Stream for EncoderMiddleware<S>
where
	S: Stream,
{
	type Item = S::Item;
	type Error = S::Error;

	#[inline]
	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		self.raw_sink.poll()
	}
}
