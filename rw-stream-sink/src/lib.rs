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
//! byte arrays, and implements `AsyncRead` and `AsyncWrite`.

extern crate bytes;
extern crate futures;
extern crate tokio_io;

use std::cmp;
use std::io::Error as IoError;
use std::io::ErrorKind as IoErrorKind;
use std::io::{Read, Write};
use bytes::{Buf, IntoBuf};
use futures::{Async, AsyncSink, Poll, Stream, Sink};
use tokio_io::{AsyncRead, AsyncWrite};

/// Wraps around a `Stream + Sink` whose items are buffers. Implements `AsyncRead` and `AsyncWrite`.
pub struct RwStreamSink<S> where S: Stream, S::Item: IntoBuf {
    inner: S,
    current_item: Option<<S::Item as IntoBuf>::Buf>,
}

impl<S> RwStreamSink<S> where S: Stream, S::Item: IntoBuf {
    /// Wraps around `inner`.
    #[inline]
    pub fn new(inner: S) -> RwStreamSink<S> {
        RwStreamSink {
            inner: inner,
            current_item: None,
        }
    }
}

impl<S> Read for RwStreamSink<S>
    where S: Stream<Error = IoError>,
          S::Item: IntoBuf,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let mut written = 0;

        loop {
            let need_new_item = if let Some(ref i) = self.current_item {
                !i.has_remaining()
            } else {
                true
            };

            if need_new_item {
                self.current_item = match self.inner.poll() {
                    Ok(Async::Ready(i)) => i.map(|b| b.into_buf()),
                    Ok(Async::NotReady) => {
                        if written == 0 {
                            return Err(IoError::new(IoErrorKind::WouldBlock, "stream not ready"));
                        } else {
                            return Ok(written);
                        }
                    },
                    Err(err) => {
                        if written == 0 {
                            return Err(err);
                        } else {
                            return Ok(written);
                        }
                    },
                };
            }

            let current_item = match self.current_item {
                Some(ref mut i) => i,
                None => return Ok(written),
            };

            let to_copy = cmp::min(buf.len() - written, current_item.remaining());
            if to_copy == 0 {
                return Ok(written);
            }

            current_item.by_ref().take(to_copy).copy_to_slice(&mut buf[written..(written+to_copy)]);
            written += to_copy;
        }
    }
}

impl<S> AsyncRead for RwStreamSink<S>
    where S: Stream<Error = IoError>,
          S::Item: IntoBuf
{
}

impl<S> Write for RwStreamSink<S>
    where S: Stream + Sink<SinkError = IoError>,
          S::SinkItem: for<'r> From<&'r [u8]>,
          S::Item: IntoBuf
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        let len = buf.len();
        match self.inner.start_send(buf.into())? {
            AsyncSink::Ready => Ok(len),
            AsyncSink::NotReady(_) => Err(IoError::new(IoErrorKind::WouldBlock, "not ready")),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        self.inner.poll_complete()?;
        Ok(())
    }
}

impl<S> AsyncWrite for RwStreamSink<S>
    where S: Stream + Sink<SinkError = IoError>,
          S::SinkItem: for<'r> From<&'r [u8]>,
          S::Item: IntoBuf
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        self.inner.poll_complete()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures::{Sink, Stream, Future, Poll, StartSend};
    use futures::sync::mpsc::channel;
    use std::io::Read;
    use RwStreamSink;

    // This struct merges a stream and a sink and is quite useful for tests.
    struct Wrapper<St, Si>(St, Si);
    impl<St, Si> Stream for Wrapper<St, Si> where St: Stream {
        type Item = St::Item;
        type Error = St::Error;
        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            self.0.poll()
        }
    }
    impl<St, Si> Sink for Wrapper<St, Si> where Si: Sink {
        type SinkItem = Si::SinkItem;
        type SinkError = Si::SinkError;
        fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
            self.1.start_send(item)
        }
        fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
            self.1.poll_complete()
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
            .wait().unwrap();

        let mut data1 = [0u8; 5];
        assert_eq!(wrapper.read(&mut data1).unwrap(), 5);
        assert_eq!(&data1, b"hello");

        let mut data2 = Vec::new();
        wrapper.read_to_end(&mut data2).unwrap();
        assert_eq!(data2, b" world");
    }
}
