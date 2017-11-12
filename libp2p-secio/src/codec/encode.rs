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

//! Individual messages encoding.

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
    where S: Sink<SinkItem = BytesMut>
{
	type SinkItem = BytesMut;
	type SinkError = S::SinkError;

	fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
		let capacity = item.len() + self.hmac_key.digest_algorithm().output_len;

		let mut out_buffer = BytesMut::with_capacity(capacity);
		// Note: Alternatively to `extend`, we could also call `advance_mut()`, which will add
		//		 uninitialized bytes to the buffer. But that's unsafe.
		out_buffer.extend((0..item.len()).map(|_| 0));
		self.cipher_state.process(&item, &mut out_buffer);

		let signature = hmac::sign(&self.hmac_key, &out_buffer);
		out_buffer.put_slice(signature.as_ref());

		debug_assert_eq!(out_buffer.len(), capacity);

		self.raw_sink.start_send(out_buffer)
	}

	#[inline]
	fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
		self.raw_sink.poll_complete()
	}
}

impl<S> Stream for EncoderMiddleware<S>
    where S: Stream
{
	type Item = S::Item;
	type Error = S::Error;

	#[inline]
	fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
		self.raw_sink.poll()
	}
}
