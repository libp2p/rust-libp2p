// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

//! This crate provides the [`RwStreamSink`] type. It wraps around a [`Stream`]
//! and [`Sink`] that produces and accepts byte arrays, and implements
//! [`AsyncRead`] and [`AsyncWrite`].
//!
//! Each call to [`AsyncWrite::poll_write`] will send one packet to the sink.
//! Calls to [`AsyncRead::poll_read`] will read from the stream's incoming packets.

use futures::{prelude::*, ready};
use std::{
    io::{self, Read},
    mem,
    pin::Pin,
    task::{Context, Poll},
};

static_assertions::const_assert!(mem::size_of::<usize>() <= mem::size_of::<u64>());

/// Wraps a [`Stream`] and [`Sink`] whose items are buffers.
/// Implements [`AsyncRead`] and [`AsyncWrite`].
#[pin_project::pin_project]
pub struct RwStreamSink<S: TryStream> {
    #[pin]
    inner: S,
    current_item: Option<std::io::Cursor<<S as TryStream>::Ok>>,
}

impl<S: TryStream> RwStreamSink<S> {
    /// Wraps around `inner`.
    pub fn new(inner: S) -> Self {
        RwStreamSink {
            inner,
            current_item: None,
        }
    }
}

impl<S> AsyncRead for RwStreamSink<S>
where
    S: TryStream<Error = io::Error>,
    <S as TryStream>::Ok: AsRef<[u8]>,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let mut this = self.project();

        // Grab the item to copy from.
        let item_to_copy = loop {
            if let Some(ref mut i) = this.current_item {
                if i.position() < i.get_ref().as_ref().len() as u64 {
                    break i;
                }
            }
            *this.current_item = Some(match ready!(this.inner.as_mut().try_poll_next(cx)) {
                Some(Ok(i)) => std::io::Cursor::new(i),
                Some(Err(e)) => return Poll::Ready(Err(e)),
                None => return Poll::Ready(Ok(0)), // EOF
            });
        };

        // Copy it!
        Poll::Ready(Ok(item_to_copy.read(buf)?))
    }
}

impl<S> AsyncWrite for RwStreamSink<S>
where
    S: TryStream + Sink<<S as TryStream>::Ok, Error = io::Error>,
    <S as TryStream>::Ok: for<'r> From<&'r [u8]>,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        let mut this = self.project();
        ready!(this.inner.as_mut().poll_ready(cx)?);
        let n = buf.len();
        if let Err(e) = this.inner.start_send(buf.into()) {
            return Poll::Ready(Err(e));
        }
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let this = self.project();
        this.inner.poll_close(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::RwStreamSink;
    use async_std::task;
    use futures::{channel::mpsc, prelude::*, stream};
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    // This struct merges a stream and a sink and is quite useful for tests.
    struct Wrapper<St, Si>(St, Si);

    impl<St, Si> Stream for Wrapper<St, Si>
    where
        St: Stream + Unpin,
        Si: Unpin,
    {
        type Item = St::Item;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
            self.0.poll_next_unpin(cx)
        }
    }

    impl<St, Si, T> Sink<T> for Wrapper<St, Si>
    where
        St: Unpin,
        Si: Sink<T> + Unpin,
    {
        type Error = Si::Error;

        fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
            Pin::new(&mut self.1).poll_ready(cx)
        }

        fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
            Pin::new(&mut self.1).start_send(item)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
            Pin::new(&mut self.1).poll_flush(cx)
        }

        fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
            Pin::new(&mut self.1).poll_close(cx)
        }
    }

    #[test]
    fn basic_reading() {
        let (tx1, _) = mpsc::channel::<Vec<u8>>(10);
        let (mut tx2, rx2) = mpsc::channel(10);

        let mut wrapper = RwStreamSink::new(Wrapper(rx2.map(Ok), tx1));

        task::block_on(async move {
            tx2.send(Vec::from("hel")).await.unwrap();
            tx2.send(Vec::from("lo wor")).await.unwrap();
            tx2.send(Vec::from("ld")).await.unwrap();
            tx2.close().await.unwrap();

            let mut data = Vec::new();
            wrapper.read_to_end(&mut data).await.unwrap();
            assert_eq!(data, b"hello world");
        })
    }

    #[test]
    fn skip_empty_stream_items() {
        let data: Vec<&[u8]> = vec![b"", b"foo", b"", b"bar", b"", b"baz", b""];
        let mut rws = RwStreamSink::new(stream::iter(data).map(Ok));
        let mut buf = [0; 9];
        task::block_on(async move {
            assert_eq!(3, rws.read(&mut buf).await.unwrap());
            assert_eq!(3, rws.read(&mut buf[3..]).await.unwrap());
            assert_eq!(3, rws.read(&mut buf[6..]).await.unwrap());
            assert_eq!(0, rws.read(&mut buf).await.unwrap());
            assert_eq!(b"foobarbaz", &buf[..])
        })
    }

    #[test]
    fn partial_read() {
        let data: Vec<&[u8]> = vec![b"hell", b"o world"];
        let mut rws = RwStreamSink::new(stream::iter(data).map(Ok));
        let mut buf = [0; 3];
        task::block_on(async move {
            assert_eq!(3, rws.read(&mut buf).await.unwrap());
            assert_eq!(b"hel", &buf[..3]);
            assert_eq!(0, rws.read(&mut buf[..0]).await.unwrap());
            assert_eq!(1, rws.read(&mut buf).await.unwrap());
            assert_eq!(b"l", &buf[..1]);
            assert_eq!(3, rws.read(&mut buf).await.unwrap());
            assert_eq!(b"o w", &buf[..3]);
            assert_eq!(0, rws.read(&mut buf[..0]).await.unwrap());
            assert_eq!(3, rws.read(&mut buf).await.unwrap());
            assert_eq!(b"orl", &buf[..3]);
            assert_eq!(1, rws.read(&mut buf).await.unwrap());
            assert_eq!(b"d", &buf[..1]);
            assert_eq!(0, rws.read(&mut buf).await.unwrap());
        })
    }
}
