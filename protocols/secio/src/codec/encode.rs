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
use super::{Hmac, StreamCipher};
use futures::prelude::*;

/// Wraps around a `Sink`. Encodes the buffers passed to it and passes it to the underlying sink.
///
/// This struct implements `Sink`. It expects individual frames of data, and outputs individual
/// frames as well, most notably without the length prefix. The mechanism for adding the length
/// prefix is not covered by this module.
///
/// Also implements `Stream` for convenience.
pub struct EncoderMiddleware<S> {
    cipher_state: StreamCipher,
    hmac: Hmac,
    raw_sink: S,
    pending: Option<BytesMut> // buffer encrypted data which can not be sent right away
}

impl<S> EncoderMiddleware<S> {
    pub fn new(raw: S, cipher: StreamCipher, hmac: Hmac) -> EncoderMiddleware<S> {
        EncoderMiddleware {
            cipher_state: cipher,
            hmac,
            raw_sink: raw,
            pending: None
        }
    }
}

impl<S> Sink for EncoderMiddleware<S>
where
    S: Sink<SinkItem = BytesMut>,
{
    type SinkItem = BytesMut;
    type SinkError = S::SinkError;

    fn start_send(&mut self, mut data_buf: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        if let Some(data) = self.pending.take() {
            if let AsyncSink::NotReady(data) = self.raw_sink.start_send(data)? {
                self.pending = Some(data);
                return Ok(AsyncSink::NotReady(data_buf))
            }
        }
        debug_assert!(self.pending.is_none());
        // TODO if SinkError gets refactor to SecioError, then use try_apply_keystream
        self.cipher_state.encrypt(&mut data_buf[..]);
        let signature = self.hmac.sign(&data_buf[..]);
        data_buf.extend_from_slice(signature.as_ref());
        if let AsyncSink::NotReady(data) = self.raw_sink.start_send(data_buf)? {
            self.pending = Some(data)
        }
        Ok(AsyncSink::Ready)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        if let Some(data) = self.pending.take() {
            if let AsyncSink::NotReady(data) = self.raw_sink.start_send(data)? {
                self.pending = Some(data);
                return Ok(Async::NotReady)
            }
        }
        self.raw_sink.poll_complete()
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        if let Some(data) = self.pending.take() {
            if let AsyncSink::NotReady(data) = self.raw_sink.start_send(data)? {
                self.pending = Some(data);
                return Ok(Async::NotReady)
            }
        }
        self.raw_sink.close()
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
