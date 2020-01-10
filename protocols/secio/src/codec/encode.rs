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

use super::{Hmac, StreamCipher};
use futures::prelude::*;
use std::{pin::Pin, task::Context, task::Poll};

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
}

impl<S> EncoderMiddleware<S> {
    pub fn new(raw: S, cipher: StreamCipher, hmac: Hmac) -> EncoderMiddleware<S> {
        EncoderMiddleware {
            cipher_state: cipher,
            hmac,
            raw_sink: raw,
        }
    }
}

impl<S> Sink<Vec<u8>> for EncoderMiddleware<S>
where
    S: Sink<Vec<u8>> + Unpin,
{
    type Error = S::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.raw_sink), cx)
    }

    fn start_send(mut self: Pin<&mut Self>, mut data_buf: Vec<u8>) -> Result<(), Self::Error> {
        // TODO if SinkError gets refactor to SecioError, then use try_apply_keystream
        self.cipher_state.encrypt(&mut data_buf[..]);
        let signature = self.hmac.sign(&data_buf[..]);
        data_buf.extend_from_slice(signature.as_ref());
        Sink::start_send(Pin::new(&mut self.raw_sink), data_buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.raw_sink), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.raw_sink), cx)
    }
}

impl<S> Stream for EncoderMiddleware<S>
where
    S: Stream + Unpin,
{
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.raw_sink), cx)
    }
}
