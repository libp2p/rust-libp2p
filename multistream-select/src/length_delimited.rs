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

//! Alternative implementation for `tokio_io::codec::length_delimited::FramedRead` with an
//! additional property: the `into_inner()` method is guarateed not to drop any data.
//!
//! Also has the length field length hardcoded.
//! 
//! We purposely only support a frame length of under 64kiB. Frames most consist in a short
//! protocol name, which is highly unlikely to be more than 64kiB long.

use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::marker::PhantomData;
use futures::{Async, StartSend, Poll, Sink, Stream};
use smallvec::SmallVec;
use tokio_io::AsyncRead;

/// Wraps around a `AsyncRead` and implements `Stream`.
///
/// Also implements `Sink` if the inner object implements `Sink`, for convenience.
///
/// The `I` generic indicates the type of data that needs to be produced by the `Stream`.
pub struct LengthDelimitedFramedRead<I, S> {
    // The inner socket where data is pulled from.
    inner: S,
    // Intermediary buffer where we put either the length of the next frame of data, or the frame
    // of data itself before it is returned.
    // Must always contain enough space to read data from `inner`.
    internal_buffer: SmallVec<[u8; 64]>,
    // Number of bytes within `internal_buffer` that contain valid data.
    internal_buffer_pos: usize,
    // State of the decoder.
    state: State,
    marker: PhantomData<I>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum State {
    // We are currently reading the length of the next frame of data.
    ReadingLength,
    // We are currently reading the frame of data itself.
    ReadingData {
        frame_len: u16
    },
}

impl<I, S> LengthDelimitedFramedRead<I, S> {
    pub fn new(inner: S) -> LengthDelimitedFramedRead<I, S> {
        LengthDelimitedFramedRead {
            inner: inner,
            internal_buffer: { let mut v = SmallVec::new(); v.push(0); v },
            internal_buffer_pos: 0,
            state: State::ReadingLength,
            marker: PhantomData,
        }
    }

    /// Destroys the `LengthDelimitedFramedRead` and returns the underlying socket.
    ///
    /// Contrary to its equivalent `tokio_io::codec::length_delimited::FramedRead`, this method is
    /// guaranteed not to skip any data from the socket.
    ///
    /// # Panic
    ///
    /// Will panic if called while there is data inside the buffer. **This can only happen if
    /// you call `poll()` manually**. Using this struct as it is intended to be used (ie. through
    /// the modifiers provided by the `futures` crate) will always leave the object in a state in
    /// which `into_inner()` will not panic.
    #[inline]
    pub fn into_inner(self) -> S {
        assert_eq!(self.state, State::ReadingLength);
        assert_eq!(self.internal_buffer_pos, 0);
        self.inner
    }
}

impl<I, S> Stream for LengthDelimitedFramedRead<I, S>
    where S: AsyncRead,
          I: for<'r> From<&'r [u8]>
{
    type Item = I;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            debug_assert!(!self.internal_buffer.is_empty());
            debug_assert!(self.internal_buffer_pos < self.internal_buffer.len());

            match self.state {
                State::ReadingLength => {
                    match self.inner.read(&mut self.internal_buffer[self.internal_buffer_pos..]) {
                        Ok(0) => {
                            // EOF
                            if self.internal_buffer_pos == 0 {
                                return Ok(Async::Ready(None));
                            } else {
                                return Err(IoError::new(IoErrorKind::BrokenPipe,
                                                        "unexpected eof"));
                            }
                        },
                        Ok(n) => {
                            debug_assert_eq!(n, 1);
                            self.internal_buffer_pos += n;
                        },
                        Err(ref err) if err.kind() == IoErrorKind::WouldBlock => {
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            return Err(err);
                        },
                    };

                    debug_assert_eq!(self.internal_buffer.len(), self.internal_buffer_pos);

                    if (*self.internal_buffer.last().unwrap_or(&0) & 0x80) == 0 {
                        // End of length prefix. Most of the time we will switch to reading data,
                        // but we need to handle a few corner cases first.
                        let frame_len = decode_length_prefix(&self.internal_buffer);

                        if frame_len >= 1 {
                            self.state = State::ReadingData { frame_len: frame_len };
                            self.internal_buffer.clear();
                            self.internal_buffer.reserve(frame_len as usize);
                            self.internal_buffer.extend((0 .. frame_len).map(|_| 0));
                            self.internal_buffer_pos = 0;

                        } else {
                            debug_assert_eq!(frame_len, 0);
                            self.state = State::ReadingLength;
                            self.internal_buffer.clear();
                            self.internal_buffer.push(0);
                            self.internal_buffer_pos = 0;
                            return Ok(Async::Ready(Some(From::from(&[][..]))));
                        }

                    } else if self.internal_buffer_pos >= 2 {
                        // Length prefix is too long. See module doc for info about max frame len.
                        return Err(IoError::new(IoErrorKind::InvalidData, "frame length too long"));

                    } else {
                        // Prepare for next read.
                        self.internal_buffer.push(0);
                    }
                },

                State::ReadingData { frame_len } => {
                    match self.inner.read(&mut self.internal_buffer[self.internal_buffer_pos..]) {
                        Ok(0) => {
                            return Err(IoError::new(IoErrorKind::BrokenPipe, "unexpected eof"));
                        },
                        Ok(n) => self.internal_buffer_pos += n,
                        Err(ref err) if err.kind() == IoErrorKind::WouldBlock => {
                            return Ok(Async::NotReady);
                        },
                        Err(err) => {
                            return Err(err);
                        },
                    };

                    if self.internal_buffer_pos >= frame_len as usize {
                        // Finished reading the frame of data.
                        self.state = State::ReadingLength;
                        let out_data = From::from(&self.internal_buffer[..]);
                        self.internal_buffer.clear();
                        self.internal_buffer.push(0);
                        self.internal_buffer_pos = 0;
                        return Ok(Async::Ready(Some(out_data)));
                    }
                },
            }
        }
    }
}

impl<I, S> Sink for LengthDelimitedFramedRead<I, S>
    where S: Sink
{
    type SinkItem = S::SinkItem;
    type SinkError = S::SinkError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.inner.start_send(item)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.poll_complete()
    }
}

fn decode_length_prefix(buf: &[u8]) -> u16 {
    debug_assert!(buf.len() <= 2);

    let mut sum = 0u16;

    for &byte in buf.iter().rev() {
        let byte = byte & 0x7f;
        sum <<= 7;
        debug_assert!(sum.checked_add(byte as u16).is_some());
        sum += byte as u16;
    }

    sum
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::io::ErrorKind;
    use futures::{Future, Stream};
    use length_delimited::LengthDelimitedFramedRead;

    #[test]
    fn basic_read() {
        let data = vec![6, 9, 8, 7, 6, 5, 4];
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.collect().wait().unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4]]);
    }

    #[test]
    fn basic_read_two() {
        let data = vec![6, 9, 8, 7, 6, 5, 4, 3, 9, 8, 7];
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.collect().wait().unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4], vec![9, 8, 7]]);
    }

    #[test]
    fn two_bytes_long_packet() {
        let len = 5000u16;
        assert!(len < (1 << 15));
        let frame = (0 .. len).map(|n| (n & 0xff) as u8).collect::<Vec<_>>();
        let mut data = vec![(len & 0x7f) as u8 | 0x80, (len >> 7) as u8];
        data.extend(frame.clone().into_iter());
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.into_future().map(|(m, _)| m).map_err(|_| ()).wait().unwrap();
        assert_eq!(recved.unwrap(), frame);
    }

    #[test]
    fn packet_len_too_long() {
        let mut data = vec![0x81, 0x81, 0x1];
        data.extend((0 .. 16513).map(|_| 0));
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.into_future().map(|(m, _)| m).map_err(|(err, _)| err).wait();
        match recved {
            Err(io_err) => assert_eq!(io_err.kind(), ErrorKind::InvalidData),
            _ => panic!()
        }
    }

    #[test]
    fn empty_frames() {
        let data = vec![0, 0, 6, 9, 8, 7, 6, 5, 4, 0, 3, 9, 8, 7];
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.collect().wait().unwrap();
        assert_eq!(recved, vec![vec![], vec![], vec![9, 8, 7, 6, 5, 4], vec![], vec![9, 8, 7]]);
    }

    #[test]
    fn unexpected_eof_in_len() {
        let data = vec![0x89];
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.collect().wait();
        match recved {
            Err(io_err) => assert_eq!(io_err.kind(), ErrorKind::BrokenPipe),
            _ => panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data() {
        let data = vec![5];
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.collect().wait();
        match recved {
            Err(io_err) => assert_eq!(io_err.kind(), ErrorKind::BrokenPipe),
            _ => panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data2() {
        let data = vec![5, 9, 8, 7];
        let framed = LengthDelimitedFramedRead::<Vec<u8>, _>::new(Cursor::new(data));

        let recved = framed.collect().wait();
        match recved {
            Err(io_err) => assert_eq!(io_err.kind(), ErrorKind::BrokenPipe),
            _ => panic!()
        }
    }
}
