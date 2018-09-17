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

//! Individual messages decoding.

use bytes::BytesMut;
use super::StreamCipher;

use error::SecioError;
use futures::sink::Sink;
use futures::stream::Stream;
use futures::Async;
use futures::Poll;
use futures::StartSend;
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
    cipher_state: StreamCipher,
    hmac_key: hmac::VerificationKey,
    // TODO: when a new version of ring is released, we can use `hmac_key.digest_algorithm().output_len` instead
    hmac_num_bytes: usize,
    raw_stream: S,
}

impl<S> DecoderMiddleware<S> {
    #[inline]
    pub fn new(
        raw_stream: S,
        cipher: StreamCipher,
        hmac_key: hmac::VerificationKey,
        hmac_num_bytes: usize, // TODO: remove this parameter
    ) -> DecoderMiddleware<S> {
        DecoderMiddleware {
            cipher_state: cipher,
            hmac_key,
            raw_stream,
            hmac_num_bytes,
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

        // TODO: when a new version of ring is released, we can use `hmac_key.digest_algorithm().output_len` instead
        let hmac_num_bytes = self.hmac_num_bytes;

        if frame.len() < hmac_num_bytes {
            debug!("frame too short when decoding secio frame");
            return Err(SecioError::FrameTooShort);
        }
        let content_length = frame.len() - hmac_num_bytes;
        {
            let (crypted_data, expected_hash) = frame.split_at(content_length);
            debug_assert_eq!(expected_hash.len(), hmac_num_bytes);

            if hmac::verify(&self.hmac_key, crypted_data, expected_hash).is_err() {
                debug!("hmac mismatch when decoding secio frame");
                return Err(SecioError::HmacNotMatching);
            }
        }

        let mut data_buf = frame.to_vec();
        data_buf.truncate(content_length);
        self.cipher_state
            .try_apply_keystream(&mut data_buf)
            .map_err::<SecioError,_>(|e|e.into())?;

        Ok(Async::Ready(Some(data_buf)))
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

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.raw_stream.close()
    }
}
