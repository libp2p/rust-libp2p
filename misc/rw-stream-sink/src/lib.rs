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

//! This crate provides the `RwStreamSink` type. It wraps around a `Stream + Sink` that produces
//! and accepts byte arrays, and implements `PollRead` and `PollWrite`.
//!
//! Each call to `write()` will send one packet on the sink. Calls to `read()` will read from
//! incoming packets.
//!
//! > **Note**: Although this crate is hosted in the libp2p repo, it is purely a utility crate and
//! >           not at all specific to libp2p.

use futures::{prelude::*, io::Initializer};
use std::{cmp, io, marker::PhantomData, pin::Pin, task::Context, task::Poll};

/// Wraps around a `Stream + Sink` whose items are buffers. Implements `AsyncRead` and `AsyncWrite`.
///
/// The `B` generic is the type of buffers that the `Sink` accepts. The `I` generic is the type of
/// buffer that the `Stream` generates.
pub struct RwStreamSink<S> {
    inner: S,
    current_item: Option<Vec<u8>>,
}

impl<S> RwStreamSink<S> {
    /// Wraps around `inner`.
    pub fn new(inner: S) -> RwStreamSink<S> {
        RwStreamSink { inner, current_item: None }
    }
}

impl<S> AsyncRead for RwStreamSink<S>
where
    S: TryStream<Ok = Vec<u8>, Error = io::Error> + Unpin,
{
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<Result<usize, io::Error>> {
        // Grab the item to copy from.
        let current_item = loop {
            if let Some(ref mut i) = self.current_item {
                if !i.is_empty() {
                    break i;
                }
            }

            self.current_item = Some(match TryStream::try_poll_next(Pin::new(&mut self.inner), cx) {
                Poll::Ready(Some(Ok(i))) => i,
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Err(err)),
                Poll::Ready(None) => return Poll::Ready(Ok(0)),     // EOF
                Poll::Pending => return Poll::Pending,
            });
        };

        // Copy it!
        debug_assert!(!current_item.is_empty());
        let to_copy = cmp::min(buf.len(), current_item.len());
        buf[..to_copy].copy_from_slice(&current_item[..to_copy]);
        for _ in 0..to_copy { current_item.remove(0); }
        Poll::Ready(Ok(to_copy))
    }

    unsafe fn initializer(&self) -> Initializer {
        Initializer::nop()
    }
}

impl<S> AsyncWrite for RwStreamSink<S>
where
    S: Stream + Sink<Vec<u8>, Error = io::Error> + Unpin,
{
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        match Sink::poll_ready(Pin::new(&mut self.inner), cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err))
        }

        let len = buf.len();
        match Sink::start_send(Pin::new(&mut self.inner), buf.into()) {
            Ok(()) => Poll::Ready(Ok(len)),
            Err(err) => Poll::Ready(Err(err))
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Sink::poll_flush(Pin::new(&mut self.inner), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Sink::poll_close(Pin::new(&mut self.inner), cx)
    }
}

impl<S> Unpin for RwStreamSink<S> {
}

#[cfg(test)]
mod tests {
    use crate::RwStreamSink;
    use futures::{prelude::*, stream, channel::mpsc::channel};
    use std::io::Read;

    // This struct merges a stream and a sink and is quite useful for tests.
    struct Wrapper<St, Si>(St, Si);
    impl<St, Si> Stream for Wrapper<St, Si>
    where
        St: Stream,
    {
        type Item = St::Item;
        type Error = St::Error;
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            self.0.poll()
        }
    }
    impl<St, Si> Sink for Wrapper<St, Si>
    where
        Si: Sink,
    {
        type SinkItem = Si::SinkItem;
        type SinkError = Si::SinkError;
        fn start_send(
            &mut self,
            item: Self::SinkItem,
        ) -> StartSend<Self::SinkItem, Self::SinkError> {
            self.1.start_send(item)
        }
        fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
            self.1.poll_complete()
        }
        fn close(&mut self) -> Poll<(), Self::SinkError> {
            self.1.close()
        }
    }

    #[test]
    fn basic_reading() {
        let (tx1, _) = channel::<Vec<u8>>(10);
        let (tx2, rx2) = channel(10);

        let mut wrapper = RwStreamSink::new(Wrapper(rx2.map_err(|_| panic!()), tx1));

        tx2.send(Bytes::from("hel"))
            .and_then(|tx| tx.send(Bytes::from("lo wor")))
            .and_then(|tx| tx.send(Bytes::from("ld")))
            .wait()
            .unwrap();

        let mut data = Vec::new();
        wrapper.read_to_end(&mut data).unwrap();
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn skip_empty_stream_items() {
        let data: Vec<&[u8]> = vec![b"", b"foo", b"", b"bar", b"", b"baz", b""];
        let mut rws = RwStreamSink::new(stream::iter_ok::<_, std::io::Error>(data));
        let mut buf = [0; 9];
        assert_eq!(3, rws.read(&mut buf).unwrap());
        assert_eq!(3, rws.read(&mut buf[3..]).unwrap());
        assert_eq!(3, rws.read(&mut buf[6..]).unwrap());
        assert_eq!(0, rws.read(&mut buf).unwrap());
        assert_eq!(b"foobarbaz", &buf[..]);
    }

    #[test]
    fn partial_read() {
        let data: Vec<&[u8]> = vec![b"hell", b"o world"];
        let mut rws = RwStreamSink::new(stream::iter_ok::<_, std::io::Error>(data));
        let mut buf = [0; 3];
        assert_eq!(3, rws.read(&mut buf).unwrap());
        assert_eq!(b"hel", &buf[..3]);
        assert_eq!(0, rws.read(&mut buf[..0]).unwrap());
        assert_eq!(1, rws.read(&mut buf).unwrap());
        assert_eq!(b"l", &buf[..1]);
        assert_eq!(3, rws.read(&mut buf).unwrap());
        assert_eq!(b"o w", &buf[..3]);
        assert_eq!(0, rws.read(&mut buf[..0]).unwrap());
        assert_eq!(3, rws.read(&mut buf).unwrap());
        assert_eq!(b"orl", &buf[..3]);
        assert_eq!(1, rws.read(&mut buf).unwrap());
        assert_eq!(b"d", &buf[..1]);
        assert_eq!(0, rws.read(&mut buf).unwrap());
    }
}
