// Copyright (c) 2018 Tokio Contributors
//
// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

// Based on copy.rs from tokio-io in https://github.com/tokio-rs/tokio

use futures::{Future, Poll, try_ready};
use std::io;
use tokio_io::{AsyncRead, AsyncWrite};

/// A future which will copy all data from a reader into a writer.
///
/// Created by the [`copy`] function, this future will resolve to the number of
/// bytes copied or an error if one happens.
///
/// [`copy`]: fn.copy.html
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct Copy<R, W> {
    reader: Option<R>,
    read_done: bool,
    flush_done: bool,
    writer: Option<W>,
    pos: usize,
    cap: usize,
    amt: u64,
    buf: Box<[u8]>,
}

/// Creates a future which represents copying all the bytes from one object to
/// another.
///
/// The returned future will copy all the bytes read from `reader` into the
/// `writer` specified. This future will only complete once the `reader` has hit
/// EOF and all bytes have been written to and flushed from the `writer`
/// provided.
///
/// On success the number of bytes is returned and the `reader` and `writer` are
/// consumed. On error the error is returned and the I/O objects are consumed as
/// well.
pub fn flushing_copy<R, W>(reader: R, writer: W) -> Copy<R, W>
    where R: AsyncRead,
          W: AsyncWrite,
{
    Copy {
        reader: Some(reader),
        read_done: false,
        flush_done: true,
        writer: Some(writer),
        amt: 0,
        pos: 0,
        cap: 0,
        buf: Box::new([0; 2048]),
    }
}

impl<R, W> Future for Copy<R, W>
    where R: AsyncRead,
          W: AsyncWrite,
{
    type Item = (u64, R, W);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(u64, R, W), io::Error> {
        debug_assert!(self.reader.is_some() && self.writer.is_some(),
            "poll() has been called again after returning Ok");

        loop {
            // Still not finished flushing
            if !self.flush_done {
                try_ready!(self.writer.as_mut().unwrap().poll_flush());
                self.flush_done = true
            }

            // If our buffer is empty, then we need to read some data to
            // continue.
            if self.pos == self.cap && !self.read_done {
                let reader = self.reader.as_mut().unwrap();
                let n = try_ready!(reader.poll_read(&mut self.buf));
                if n == 0 {
                    self.read_done = true;
                } else {
                    self.pos = 0;
                    self.cap = n;
                }
            }

            // If our buffer has some data, let's write it out!
            while self.pos < self.cap {
                let writer = self.writer.as_mut().unwrap();
                let i = try_ready!(writer.poll_write(&self.buf[self.pos..self.cap]));
                if i == 0 {
                    return Err(io::Error::new(io::ErrorKind::WriteZero,
                                              "write zero byte into writer"));
                } else {
                    self.pos += i;
                    self.amt += i as u64;
                }
            }

            // The buffered data has been written, let's flush it!
            if self.pos == self.cap && !self.read_done {
                self.flush_done = false;
                continue
            }

            // Everything has been copied.
            if self.pos == self.cap && self.read_done {
                try_ready!(self.writer.as_mut().unwrap().poll_flush());
                let reader = self.reader.take().unwrap();
                let writer = self.writer.take().unwrap();
                return Ok((self.amt, reader, writer).into())
            }
        }
    }
}

