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

use super::{Hmac, StreamCipher};

use crate::error::SecioError;
use futures::prelude::*;
use log::debug;
use std::{cmp::min, pin::Pin, task::Context, task::Poll};

/// Wraps around a `Stream<Item = Vec<u8>>`. The buffers produced by the underlying stream
/// are decoded using the cipher and hmac.
///
/// This struct implements `Stream`, whose stream item are frames of data without the length
/// prefix. The mechanism for removing the length prefix and splitting the incoming data into
/// frames isn't handled by this module.
///
/// Also implements `Sink` for convenience.
pub struct DecoderMiddleware<S> {
    cipher_state: StreamCipher,
    hmac: Hmac,
    raw_stream: S,
    nonce: Vec<u8>
}

impl<S> DecoderMiddleware<S> {
    /// Create a new decoder for the given stream, using the provided cipher and HMAC.
    ///
    /// The `nonce` parameter denotes a sequence of bytes which are expected to be found at the
    /// beginning of the stream and are checked for equality.
    pub fn new(raw_stream: S, cipher: StreamCipher, hmac: Hmac, nonce: Vec<u8>) -> DecoderMiddleware<S> {
        DecoderMiddleware {
            cipher_state: cipher,
            hmac,
            raw_stream,
            nonce
        }
    }
}

impl<S> Stream for DecoderMiddleware<S>
where
    S: TryStream<Ok = Vec<u8>> + Unpin,
    S::Error: Into<SecioError>,
{
    type Item = Result<Vec<u8>, SecioError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let frame = match TryStream::try_poll_next(Pin::new(&mut self.raw_stream), cx) {
            Poll::Ready(Some(Ok(t))) => t,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err.into()))),
        };

        if frame.len() < self.hmac.num_bytes() {
            debug!("frame too short when decoding secio frame");
            return Poll::Ready(Some(Err(SecioError::FrameTooShort)));
        }
        let content_length = frame.len() - self.hmac.num_bytes();
        {
            let (crypted_data, expected_hash) = frame.split_at(content_length);
            debug_assert_eq!(expected_hash.len(), self.hmac.num_bytes());

            if self.hmac.verify(crypted_data, expected_hash).is_err() {
                debug!("hmac mismatch when decoding secio frame");
                return Poll::Ready(Some(Err(SecioError::HmacNotMatching)));
            }
        }

        let mut data_buf = frame;
        data_buf.truncate(content_length);
        self.cipher_state.decrypt(&mut data_buf);

        if !self.nonce.is_empty() {
            let n = min(data_buf.len(), self.nonce.len());
            if data_buf[.. n] != self.nonce[.. n] {
                return Poll::Ready(Some(Err(SecioError::NonceVerificationFailed)))
            }
            self.nonce.drain(.. n);
            data_buf.drain(.. n);
        }

        Poll::Ready(Some(Ok(data_buf)))
    }
}

impl<S, I> Sink<I> for DecoderMiddleware<S>
where
    S: Sink<I> + Unpin,
{
    type Error = S::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.raw_stream), cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.raw_stream), item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.raw_stream), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.raw_stream), cx)
    }
}
