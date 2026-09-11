// Copyright 2022 Parity Technologies (UK) Ltd.
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

use std::num::NonZeroUsize;

use asynchronous_codec::Framed;
use futures::{AsyncRead, AsyncWrite};

use crate::{
    proto::Message,
    stream::{StreamConfig, VARINT_LEN},
};

pub(crate) type FramedDc<T> = Framed<T, prost_codec::Codec<Message>>;
pub(crate) fn new<T>(inner: T, config: StreamConfig) -> FramedDc<T>
where
    T: AsyncRead + AsyncWrite,
{
    let mut framed = Framed::new(inner, codec(config));
    // One encoded frame per write, because the layer below turns every write into exactly one
    // SCTP user message — and that message must not exceed `max_message_size`.
    //
    // The high-water mark is a *lower* bound on when to flush, not an upper bound on the
    // buffer: `poll_ready` flushes while `buffer.len() >= hwm` and `start_send` then appends a
    // whole frame, so the buffer peaks at `hwm - 1 + one frame`. Any `hwm` above 1 therefore
    // lets two frames coalesce into a single message of up to `2 * max_message_size`, which
    // webrtc-rs rejects with "outbound packet larger than maximum message size" — silently
    // losing that frame.
    //
    // This used to be `config.max_data_size()`, which only ever worked because the SDP always
    // advertised a hard-coded 16 KiB while the framing layer used 8 KiB: the doubled buffer
    // landed exactly on the advertised limit. Raising the configured size, or advertising the
    // configured size honestly, broke it in both directions.
    //
    // Sending one frame per message is also what the spec describes; coalescing happened to
    // decode correctly only because each frame carries its own length prefix.
    framed.set_send_high_water_mark(1);
    framed
}

pub(crate) fn codec(config: StreamConfig) -> prost_codec::Codec<Message, Message> {
    prost_codec::Codec::new(config.max_message_size() - VARINT_LEN)
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll},
    };

    use futures::{AsyncRead, AsyncWrite, SinkExt};

    use super::*;

    /// Records the length of every individual write.
    ///
    /// The layer this sits on top of in production (`PollDataChannel`) turns one write into one
    /// SCTP user message, so these lengths *are* the message sizes the peer's SCTP will police.
    #[derive(Clone, Default)]
    struct RecordingWriter(Arc<Mutex<Vec<usize>>>);

    impl AsyncRead for RecordingWriter {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(0))
        }
    }

    impl AsyncWrite for RecordingWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.0.lock().unwrap().push(buf.len());
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// No single write may exceed `max_message_size`, whatever mix of frame sizes is queued.
    ///
    /// **The sizes must be mixed.** A run of full-size frames alone never reproduces the bug:
    /// each one lands the buffer above the high-water mark, so the next `poll_ready` flushes it
    /// and nothing is left behind. It takes a *small* frame — one that leaves the buffer below
    /// the mark — followed by a full-size one for the two to be written out together. Measured
    /// on a real transfer before the fix: 125 writes of 8190 B and three of **8419 B** against
    /// an 8192 B limit; SCTP rejected exactly those three and the stream lost them.
    #[test]
    fn no_write_exceeds_the_configured_message_size() {
        for bytes in [8 * 1024usize, 16 * 1024, 64 * 1024] {
            let config = StreamConfig::new(NonZeroUsize::new(bytes).expect("non-zero"));
            let writer = RecordingWriter::default();
            let mut framed = new(writer.clone(), config);

            futures::executor::block_on(async {
                // `feed`, not `send`: `send` flushes after every item, which empties the
                // buffer between frames and hides the coalescing entirely. The production
                // path (`Stream::poll_write`) does not flush per frame either.
                for _ in 0..4 {
                    // A short frame first: it leaves the buffer below the high-water mark.
                    framed
                        .feed(Message {
                            flag: Some(0),
                            message: Some(vec![0u8; 200]),
                        })
                        .await
                        .expect("feed");
                    framed
                        .feed(Message {
                            flag: None,
                            message: Some(vec![0u8; config.max_data_size()]),
                        })
                        .await
                        .expect("feed");
                }
                framed.close().await.expect("close");
            });

            let writes = writer.0.lock().unwrap().clone();
            assert!(!writes.is_empty(), "nothing was written");
            for len in writes {
                assert!(
                    len <= bytes,
                    "a {len} B write exceeds the {bytes} B limit; SCTP would reject it and the \
                     frame would be lost"
                );
            }
        }
    }
}
