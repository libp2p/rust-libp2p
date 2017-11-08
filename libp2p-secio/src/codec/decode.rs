use bytes::BytesMut;
use crypto::symmetriccipher::SynchronousStreamCipher;

use error::SecioError;
use futures::Async;
use futures::Poll;
use futures::StartSend;
use futures::sink::Sink;
use futures::stream::Stream;
use ring::hmac;

/// Wraps around a `Stream<Item = BytesMut>`. The buffers produced by the underlying stream
/// are decoded using the cipher and hmac.
///
/// This struct implements `Stream`, whose stream item are frames of data without the length
/// prefix. The mechanism for removing the length prefix and splitting the incoming data into
/// frames isn't handled by this module.
///
/// Also implements `Sink` for convenience.
pub struct DecoderMiddleware<S> {
	cipher_state: Box<SynchronousStreamCipher>,
	hmac_key: hmac::VerificationKey,
	raw_stream: S,
}

impl<S> DecoderMiddleware<S> {
	#[inline]
	pub fn new(
		raw_stream: S,
		cipher: Box<SynchronousStreamCipher>,
		hmac_key: hmac::VerificationKey,
	) -> DecoderMiddleware<S> {
		DecoderMiddleware {
			cipher_state: cipher,
			hmac_key: hmac_key,
			raw_stream: raw_stream,
		}
	}
}

impl<S> Stream for DecoderMiddleware<S>
where
	S: Stream<Item = BytesMut>,
	S::Error: Into<SecioError>,
{
	type Item = Vec<u8>;
	type Error = SecioError;

	#[inline]
	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		let frame = match self.raw_stream.poll() {
			Ok(Async::Ready(Some(t))) => t,
			Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
			Ok(Async::NotReady) => return Ok(Async::NotReady),
			Err(err) => return Err(err.into()),
		};

		let hmac_num_bytes = self.hmac_key.digest_algorithm().output_len;

		if frame.len() < hmac_num_bytes {
			return Err(SecioError::FrameTooShort);
		}

		let (crypted_data, expected_hash) = frame.split_at(frame.len() - hmac_num_bytes);
		debug_assert_eq!(expected_hash.len(), hmac_num_bytes);

		if let Err(_) = hmac::verify(&self.hmac_key, crypted_data, expected_hash) {
			return Err(SecioError::HmacNotMatching);
		}

		let mut decrypted_data = crypted_data.to_vec(); // TODO: don't alloc
		self.cipher_state.process(&crypted_data, &mut decrypted_data);

		Ok(Async::Ready(Some(decrypted_data)))
	}
}

impl<S> Sink for DecoderMiddleware<S>
where
	S: Sink,
{
	type SinkItem = S::SinkItem;
	type SinkError = S::SinkError;

	#[inline]
	fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
		self.raw_stream.start_send(item)
	}

	#[inline]
	fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
		self.raw_stream.poll_complete()
	}
}
