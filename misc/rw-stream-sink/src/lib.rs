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
//! and accepts byte arrays, and implements `AsyncRead` and `AsyncWrite`.
//!
//! Each call to `write()` will send one packet on the sink. Calls to `read()` will read from
//! incoming packets.
//!
//! > **Note**: Although this crate is hosted in the libp2p repo, it is purely a utility crate and
//! >           not at all specific to libp2p.

use bytes::{Buf, IntoBuf};
use futures::{Async, AsyncSink, Poll, Sink, Stream};
use std::cmp;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::{Read, Write};
use tokio_io::{AsyncRead, AsyncWrite};

/// Wraps around a `Stream + Sink` whose items are buffers. Implements `AsyncRead` and `AsyncWrite`.
pub struct RwStreamSink<S>
where
    S: Stream,
    S::Item: IntoBuf,
{
    inner: S,
    current_item: Option<<S::Item as IntoBuf>::Buf>,
}

impl<S> RwStreamSink<S>
where
    S: Stream,
    S::Item: IntoBuf,
{
    /// Wraps around `inner`.
    pub fn new(inner: S) -> RwStreamSink<S> {
        RwStreamSink { inner, current_item: None }
    }
}

impl<S> Read for RwStreamSink<S>
where
    S: Stream<Error = IoError>,
    S::Item: IntoBuf,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        // Grab the item to copy from.
        let item_to_copy = loop {
            if let Some(ref mut i) = self.current_item {
                if i.has_remaining() {
                    break i;
                }
            }

            self.current_item = Some(match self.inner.poll()? {
                Async::Ready(Some(i)) => i.into_buf(),
                Async::Ready(None) => return Ok(0),     // EOF
                Async::NotReady => return Err(IoErrorKind::WouldBlock.into()),
            });
        };

        // Copy it!
        debug_assert!(item_to_copy.has_remaining());
        let to_copy = cmp::min(buf.len(), item_to_copy.remaining());
        item_to_copy.take(to_copy).copy_to_slice(&mut buf[..to_copy]);
        Ok(to_copy)
    }
}

impl<S> AsyncRead for RwStreamSink<S>
where
    S: Stream<Error = IoError>,
    S::Item: IntoBuf,
{
    unsafe fn prepare_uninitialized_buffer(&self, _: &mut [u8]) -> bool {
        false
    }
}

impl<S> Write for RwStreamSink<S>
where
    S: Stream + Sink<SinkError = IoError>,
    S::SinkItem: for<'r> From<&'r [u8]>,
    S::Item: IntoBuf,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        let len = buf.len();
        match self.inner.start_send(buf.into())? {
            AsyncSink::Ready => Ok(len),
            AsyncSink::NotReady(_) => Err(IoError::new(IoErrorKind::WouldBlock, "not ready")),
        }
    }

    fn flush(&mut self) -> Result<(), IoError> {
        match self.inner.poll_complete()? {
            Async::Ready(()) => Ok(()),
            Async::NotReady => Err(IoError::new(IoErrorKind::WouldBlock, "not ready"))
        }
    }
}

impl<S> AsyncWrite for RwStreamSink<S>
where
    S: Stream + Sink<SinkError = IoError>,
    S::SinkItem: for<'r> From<&'r [u8]>,
    S::Item: IntoBuf,
{
    fn shutdown(&mut self) -> Poll<(), IoError> {
        self.inner.close()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use crate::RwStreamSink;
    use futures::{prelude::*, stream, sync::mpsc::channel};
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
