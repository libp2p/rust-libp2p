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

use bytes::Bytes;
use futures::{Async, Poll, Sink, StartSend, Stream};
use smallvec::SmallVec;
use std::{io, u16};
use tokio_codec::{Encoder, FramedWrite};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::decode;

/// `Stream` and `Sink` wrapping some `AsyncRead + AsyncWrite` object to read
/// and write unsigned-varint prefixed frames.
///
/// We purposely only support a frame length of under 64kiB. Frames mostly consist
/// in a short protocol name, which is highly unlikely to be more than 64kiB long.
pub struct LengthDelimited<R, C> {
    // The inner socket where data is pulled from.
    inner: FramedWrite<R, C>,
    // Intermediary buffer where we put either the length of the next frame of data, or the frame
    // of data itself before it is returned.
    // Must always contain enough space to read data from `inner`.
    internal_buffer: SmallVec<[u8; 64]>,
    // Number of bytes within `internal_buffer` that contain valid data.
    internal_buffer_pos: usize,
    // State of the decoder.
    state: State
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum State {
    // We are currently reading the length of the next frame of data.
    ReadingLength,
    // We are currently reading the frame of data itself.
    ReadingData { frame_len: u16 },
}

impl<R, C> LengthDelimited<R, C>
where
    R: AsyncWrite,
    C: Encoder
{
    pub fn new(inner: R, codec: C) -> LengthDelimited<R, C> {
        LengthDelimited {
            inner: FramedWrite::new(inner, codec),
            internal_buffer: {
                let mut v = SmallVec::new();
                v.push(0);
                v
            },
            internal_buffer_pos: 0,
            state: State::ReadingLength
        }
    }

    /// Destroys the `LengthDelimited` and returns the underlying socket.
    ///
    /// Contrary to its equivalent `tokio_io::codec::length_delimited::FramedRead`, this method is
    /// guaranteed not to skip any data from the socket.
    ///
    /// # Panic
    ///
    /// Will panic if called while there is data inside the buffer. **This can only happen if
    /// you call `poll()` manually**. Using this struct as it is intended to be used (i.e. through
    /// the modifiers provided by the `futures` crate) will always leave the object in a state in
    /// which `into_inner()` will not panic.
    #[inline]
    pub fn into_inner(self) -> R {
        assert_eq!(self.state, State::ReadingLength);
        assert_eq!(self.internal_buffer_pos, 0);
        self.inner.into_inner()
    }
}

impl<R, C> Stream for LengthDelimited<R, C>
where
    R: AsyncRead
{
    type Item = Bytes;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            debug_assert!(!self.internal_buffer.is_empty());
            debug_assert!(self.internal_buffer_pos < self.internal_buffer.len());

            match self.state {
                State::ReadingLength => {
                    let slice = &mut self.internal_buffer[self.internal_buffer_pos..];
                    match self.inner.get_mut().read(slice) {
                        Ok(0) => {
                            // EOF
                            if self.internal_buffer_pos == 0 {
                                return Ok(Async::Ready(None));
                            } else {
                                return Err(io::ErrorKind::UnexpectedEof.into());
                            }
                        }
                        Ok(n) => {
                            debug_assert_eq!(n, 1);
                            self.internal_buffer_pos += n;
                        }
                        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                            return Ok(Async::NotReady);
                        }
                        Err(err) => {
                            return Err(err);
                        }
                    };

                    debug_assert_eq!(self.internal_buffer.len(), self.internal_buffer_pos);

                    if (*self.internal_buffer.last().unwrap_or(&0) & 0x80) == 0 {
                        // End of length prefix. Most of the time we will switch to reading data,
                        // but we need to handle a few corner cases first.
                        let (frame_len, _) = decode::u16(&self.internal_buffer).map_err(|e| {
                            log::debug!("invalid length prefix: {}", e);
                            io::Error::new(io::ErrorKind::InvalidData, "invalid length prefix")
                        })?;

                        if frame_len >= 1 {
                            self.state = State::ReadingData { frame_len };
                            self.internal_buffer.clear();
                            self.internal_buffer.reserve(frame_len as usize);
                            self.internal_buffer.extend((0..frame_len).map(|_| 0));
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
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame length too long"));
                    } else {
                        // Prepare for next read.
                        self.internal_buffer.push(0);
                    }
                }
                State::ReadingData { frame_len } => {
                    let slice = &mut self.internal_buffer[self.internal_buffer_pos..];
                    match self.inner.get_mut().read(slice) {
                        Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                        Ok(n) => self.internal_buffer_pos += n,
                        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                            return Ok(Async::NotReady)
                        }
                        Err(err) => return Err(err)
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
                }
            }
        }
    }
}

impl<R, C> Sink for LengthDelimited<R, C>
where
    R: AsyncWrite,
    C: Encoder
{
    type SinkItem = <FramedWrite<R, C> as Sink>::SinkItem;
    type SinkError = <FramedWrite<R, C> as Sink>::SinkError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.inner.start_send(item)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.poll_complete()
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.close()
    }
}

#[cfg(test)]
mod tests {
    use futures::{Future, Stream};
    use crate::length_delimited::LengthDelimited;
    use std::io::{Cursor, ErrorKind};
    use unsigned_varint::codec::UviBytes;

    #[test]
    fn basic_read() {
        let data = vec![6, 9, 8, 7, 6, 5, 4];
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed.collect().wait().unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4]]);
    }

    #[test]
    fn basic_read_two() {
        let data = vec![6, 9, 8, 7, 6, 5, 4, 3, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed.collect().wait().unwrap();
        assert_eq!(recved, vec![vec![9, 8, 7, 6, 5, 4], vec![9, 8, 7]]);
    }

    #[test]
    fn two_bytes_long_packet() {
        let len = 5000u16;
        assert!(len < (1 << 15));
        let frame = (0..len).map(|n| (n & 0xff) as u8).collect::<Vec<_>>();
        let mut data = vec![(len & 0x7f) as u8 | 0x80, (len >> 7) as u8];
        data.extend(frame.clone().into_iter());
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed
            .into_future()
            .map(|(m, _)| m)
            .map_err(|_| ())
            .wait()
            .unwrap();
        assert_eq!(recved.unwrap(), frame);
    }

    #[test]
    fn packet_len_too_long() {
        let mut data = vec![0x81, 0x81, 0x1];
        data.extend((0..16513).map(|_| 0));
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed
            .into_future()
            .map(|(m, _)| m)
            .map_err(|(err, _)| err)
            .wait();

        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::InvalidData)
        } else {
            panic!()
        }
    }

    #[test]
    fn empty_frames() {
        let data = vec![0, 0, 6, 9, 8, 7, 6, 5, 4, 0, 3, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed.collect().wait().unwrap();
        assert_eq!(
            recved,
            vec![
                vec![],
                vec![],
                vec![9, 8, 7, 6, 5, 4],
                vec![],
                vec![9, 8, 7],
            ]
        );
    }

    #[test]
    fn unexpected_eof_in_len() {
        let data = vec![0x89];
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed.collect().wait();
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data() {
        let data = vec![5];
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed.collect().wait();
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }

    #[test]
    fn unexpected_eof_in_data2() {
        let data = vec![5, 9, 8, 7];
        let framed = LengthDelimited::new(Cursor::new(data), UviBytes::<Vec<_>>::default());
        let recved = framed.collect().wait();
        if let Err(io_err) = recved {
            assert_eq!(io_err.kind(), ErrorKind::UnexpectedEof)
        } else {
            panic!()
        }
    }
}

